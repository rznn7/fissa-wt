use std::io::{self, Stderr};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::components::create_form::CreateForm;
use crate::components::list::{ListComponent, Row};
use crate::components::progress::ProgressComponent;
use crate::components::{Component, KeyEventResponse};
use crate::create;
use crate::dirty;
use crate::git::{self, Repo};
use crate::node;
use crate::shell;

pub type Term = Terminal<CrosstermBackend<Stderr>>;

pub fn init_terminal() -> Result<Term> {
    enable_raw_mode()?;
    crossterm::execute!(io::stderr(), EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(io::stderr()))?)
}

pub fn restore_terminal() -> Result<()> {
    // Ratatui leaves the cursor hidden, so hand a visible one back to the shell.
    crossterm::execute!(io::stderr(), LeaveAlternateScreen, crossterm::cursor::Show)?;
    disable_raw_mode()?;
    Ok(())
}

pub fn build_rows(repo: &Repo) -> Result<Vec<Row>> {
    let mut rows = Vec::new();

    for entry in repo.worktrees()? {
        if entry.bare {
            continue;
        }
        rows.push(Row {
            label: git::row_label(&entry.path, &repo.main_clone),
            branch: entry
                .branch
                .clone()
                .unwrap_or_else(|| String::from("(detached)")),
            dirty: None,
            path: entry.path,
        });
    }

    rows.sort_by(|a, b| a.label.cmp(&b.label));
    Ok(rows)
}

pub fn preflight(dest: &Path, branch_exists: bool) -> Option<String> {
    if dest.exists() {
        return Some(String::from("directory already exists"));
    }
    if branch_exists {
        return Some(String::from("branch already exists"));
    }
    None
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProgressAction {
    None,
    Cd,
    Back,
}

pub fn progress_action(finished: bool, failed: bool, code: KeyCode) -> ProgressAction {
    if !finished {
        return ProgressAction::None;
    }
    match code {
        KeyCode::Enter if !failed => ProgressAction::Cd,
        KeyCode::Enter | KeyCode::Esc => ProgressAction::Back,
        _ => ProgressAction::None,
    }
}

fn make_list(repo: &Repo) -> Result<ListComponent> {
    let rows = build_rows(repo)?;
    Ok(ListComponent::new(
        repo.main_dir_name(),
        rows,
        shell::wrapper_active(),
    ))
}

enum Screen {
    List,
    Create,
    Progress,
}

pub struct App {
    repo: Repo,
    list: ListComponent,
    form: Option<CreateForm>,
    progress: Option<ProgressComponent>,
    receiver: Option<Receiver<create::Progress>>,
    dirty: Option<Receiver<dirty::Report>>,
    screen: Screen,
    created: Option<PathBuf>,
    chosen: Option<PathBuf>,
    quit: bool,
}

impl App {
    pub fn new(repo: Repo) -> Result<App> {
        let list = make_list(&repo)?;
        let dirty = Some(dirty::spawn(list.paths()));
        Ok(App {
            repo,
            list,
            form: None,
            progress: None,
            receiver: None,
            dirty,
            screen: Screen::List,
            created: None,
            chosen: None,
            quit: false,
        })
    }

    pub fn run(mut self, terminal: &mut Term) -> Result<Option<PathBuf>> {
        while !self.quit {
            self.drain_dirty();
            self.drain_progress();
            if let Some(progress) = self.progress.as_mut() {
                progress.tick();
            }
            self.draw(terminal)?;

            if !event::poll(Duration::from_millis(100))? {
                continue;
            }
            if let Event::Key(key_event) = event::read()?
                && key_event.kind == KeyEventKind::Press
            {
                self.on_key(key_event)?;
            }
        }
        Ok(self.chosen)
    }

    fn draw(&mut self, terminal: &mut Term) -> Result<()> {
        terminal.draw(|frame| {
            let area = frame.area();
            match self.screen {
                Screen::List => self.list.render(frame, area),
                Screen::Create => {
                    if let Some(form) = self.form.as_mut() {
                        form.render(frame, area);
                    }
                }
                Screen::Progress => {
                    if let Some(progress) = self.progress.as_mut() {
                        progress.render(frame, area);
                    }
                }
            }
        })?;
        Ok(())
    }

    fn on_key(&mut self, key_event: crossterm::event::KeyEvent) -> Result<()> {
        match self.screen {
            // The list gets first refusal: an open search bar claims every key.
            Screen::List => match self.list.handle_event_key(key_event) {
                KeyEventResponse::Consumed => {}
                KeyEventResponse::Ignored => match key_event.code {
                    KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
                    KeyCode::Enter => {
                        if let Some(path) = self.list.selected_path() {
                            self.chosen = Some(path);
                            self.quit = true;
                        }
                    }
                    KeyCode::Char('n') => self.open_form(),
                    _ => {}
                },
            },
            Screen::Create => {
                if let Some(form) = self.form.as_mut() {
                    form.handle_event_key(key_event);
                    if form.take_cancel() {
                        self.form = None;
                        self.screen = Screen::List;
                    } else if form.take_submit() {
                        self.submit_form()?;
                    }
                }
            }
            Screen::Progress => {
                let finished = self
                    .progress
                    .as_ref()
                    .map(ProgressComponent::finished)
                    .unwrap_or(false);
                let failed = self
                    .progress
                    .as_ref()
                    .and_then(ProgressComponent::failure)
                    .is_some();

                match progress_action(finished, failed, key_event.code) {
                    ProgressAction::None => {}
                    ProgressAction::Cd => {
                        self.chosen = self.created.take();
                        self.quit = true;
                    }
                    ProgressAction::Back => self.back_to_list()?,
                }
            }
        }
        Ok(())
    }

    fn dest_parent(&self) -> PathBuf {
        self.repo
            .main_clone
            .parent()
            .unwrap_or(&self.repo.main_clone)
            .to_path_buf()
    }

    fn open_form(&mut self) {
        let targets = node::discover_targets(&self.repo.source);
        let allowed = node::available_strategies(&targets);

        self.form = Some(CreateForm::new(
            self.repo.main_dir_name(),
            self.repo.prefixes(),
            self.repo.default_base(),
            allowed,
        ));
        self.screen = Screen::Create;
    }

    fn submit_form(&mut self) -> Result<()> {
        let dest_parent = self.dest_parent();
        let Some(form) = self.form.as_mut() else {
            return Ok(());
        };
        let (Some(branch), Some(dir)) = (form.branch(), form.dir()) else {
            return Ok(());
        };

        let dest = dest_parent.join(&dir);

        if !self.repo.ref_exists(form.base()) {
            form.set_error(Some(format!("unknown base ref '{}'", form.base())));
            return Ok(());
        }
        if let Some(error) = preflight(&dest, self.repo.branch_exists(&branch)) {
            form.set_error(Some(error));
            return Ok(());
        }

        let targets = node::discover_targets(&self.repo.source);
        let steps = create::plan_steps(&branch, form.base(), form.strategy(), &targets);
        let labels: Vec<String> = steps.iter().map(|step| step.label.clone()).collect();

        let (sender, receiver) = mpsc::channel();
        create::spawn(
            self.repo.source.clone(),
            create::Request {
                dest: dest.clone(),
                steps,
            },
            sender,
        );

        self.progress = Some(ProgressComponent::new(
            format!("creating {dir}"),
            labels,
            shell::wrapper_active(),
        ));
        self.receiver = Some(receiver);
        self.created = Some(dest);
        self.form = None;
        self.screen = Screen::Progress;
        Ok(())
    }

    /// The dirty markers are cosmetic, so the list draws without them and the
    /// scan streams them in rather than holding up the first frame.
    fn drain_dirty(&mut self) {
        let Some(receiver) = self.dirty.as_ref() else {
            return;
        };
        loop {
            match receiver.try_recv() {
                Ok(report) => self.list.set_dirty(&report.path, report.dirty),
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.dirty = None;
                    return;
                }
            }
        }
    }

    fn drain_progress(&mut self) {
        let Some(receiver) = self.receiver.as_ref() else {
            return;
        };
        while let Ok(progress) = receiver.try_recv() {
            if let Some(component) = self.progress.as_mut() {
                component.apply(progress);
            }
        }
    }

    fn back_to_list(&mut self) -> Result<()> {
        self.created = None;
        self.receiver = None;
        self.progress = None;
        self.list = make_list(&self.repo)?;
        self.dirty = Some(dirty::spawn(self.list.paths()));
        self.screen = Screen::List;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(cwd: &std::path::Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn repo_with_a_second_worktree(root: &std::path::Path) -> Repo {
        run(root, &["init", "--quiet", "-b", "main"]);
        std::fs::write(root.join("README.md"), "hi").unwrap();
        run(root, &["add", "README.md"]);
        run(
            root,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-qm",
                "init",
            ],
        );
        run(root, &["worktree", "add", "-q", "-b", "side", "../side"]);
        Repo::discover(root).unwrap()
    }

    fn app_on_the_list_screen(tmp: &tempfile::TempDir) -> App {
        let root = tmp.path().join("main");
        std::fs::create_dir_all(&root).unwrap();
        App::new(repo_with_a_second_worktree(&root)).unwrap()
    }

    fn press(app: &mut App, code: KeyCode) {
        app.on_key(crossterm::event::KeyEvent::new(
            code,
            crossterm::event::KeyModifiers::NONE,
        ))
        .unwrap();
    }

    fn type_query(app: &mut App, query: &str) {
        press(app, KeyCode::Char('/'));
        for character in query.chars() {
            press(app, KeyCode::Char(character));
        }
    }

    #[test]
    fn test_q_is_typed_into_the_query_instead_of_quitting() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = app_on_the_list_screen(&tmp);
        type_query(&mut app, "q");
        assert!(!app.quit);
    }

    #[test]
    fn test_n_is_typed_into_the_query_instead_of_opening_the_form() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = app_on_the_list_screen(&tmp);
        type_query(&mut app, "n");
        assert!(matches!(app.screen, Screen::List));
    }

    #[test]
    fn test_enter_commits_the_query_without_quitting() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = app_on_the_list_screen(&tmp);
        type_query(&mut app, "side");
        press(&mut app, KeyCode::Enter);
        assert!(!app.quit);
        assert_eq!(app.chosen, None);
    }

    #[test]
    fn test_esc_clears_the_filter_before_it_quits() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = app_on_the_list_screen(&tmp);
        type_query(&mut app, "side");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Esc);
        assert!(!app.quit, "the first esc should only clear the filter");
        press(&mut app, KeyCode::Esc);
        assert!(app.quit);
    }

    #[test]
    fn test_enter_cds_into_the_row_the_query_selected() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = app_on_the_list_screen(&tmp);
        type_query(&mut app, "side");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Enter);
        assert!(app.quit);
        assert!(
            app.chosen.as_ref().unwrap().ends_with("side"),
            "{:?}",
            app.chosen
        );
    }

    #[test]
    fn test_enter_does_not_quit_when_the_query_matches_no_row() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = app_on_the_list_screen(&tmp);
        type_query(&mut app, "zzz");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Enter);
        assert!(!app.quit);
        assert_eq!(app.chosen, None);
    }

    #[test]
    fn test_progress_action_ignores_keys_while_the_run_is_unfinished() {
        assert_eq!(
            progress_action(false, false, KeyCode::Enter),
            ProgressAction::None
        );
    }

    #[test]
    fn test_progress_action_cds_on_enter_after_a_successful_run() {
        assert_eq!(
            progress_action(true, false, KeyCode::Enter),
            ProgressAction::Cd
        );
    }

    #[test]
    fn test_progress_action_returns_to_the_list_on_esc_after_a_successful_run() {
        assert_eq!(
            progress_action(true, false, KeyCode::Esc),
            ProgressAction::Back
        );
    }

    #[test]
    fn test_progress_action_returns_to_the_list_on_enter_after_a_failure() {
        assert_eq!(
            progress_action(true, true, KeyCode::Enter),
            ProgressAction::Back
        );
    }

    #[test]
    fn test_progress_action_returns_to_the_list_on_esc_after_a_failure() {
        assert_eq!(
            progress_action(true, true, KeyCode::Esc),
            ProgressAction::Back
        );
    }

    #[test]
    fn test_progress_action_ignores_unrelated_keys_after_a_successful_run() {
        assert_eq!(
            progress_action(true, false, KeyCode::Char('q')),
            ProgressAction::None
        );
    }

    #[test]
    fn test_preflight_rejects_an_existing_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let existing = tmp.path().join("spectra-x");
        std::fs::create_dir_all(&existing).unwrap();
        assert_eq!(
            preflight(&existing, false),
            Some(String::from("directory already exists"))
        );
    }

    #[test]
    fn test_preflight_rejects_an_existing_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let fresh = tmp.path().join("spectra-x");
        assert_eq!(
            preflight(&fresh, true),
            Some(String::from("branch already exists"))
        );
    }

    #[test]
    fn test_preflight_accepts_a_fresh_directory_and_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let fresh = tmp.path().join("spectra-x");
        assert_eq!(preflight(&fresh, false), None);
    }

    #[test]
    fn test_build_rows_leaves_the_dirty_state_unknown() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("main");
        std::fs::create_dir_all(&root).unwrap();
        let repo = repo_with_a_second_worktree(&root);
        std::fs::write(root.join("scratch.txt"), "untracked").unwrap();

        let rows = build_rows(&repo).unwrap();
        assert!(
            rows.iter().all(|row| row.dirty.is_none()),
            "startup must not pay for a git status per worktree"
        );
    }

    #[test]
    fn test_build_rows_lists_every_worktree_with_a_label() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("main");
        std::fs::create_dir_all(&root).unwrap();
        let repo = repo_with_a_second_worktree(&root);

        let rows = build_rows(&repo).unwrap();
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(rows.len(), 2);
        assert!(labels.contains(&"side"), "{labels:?}");
    }
}
