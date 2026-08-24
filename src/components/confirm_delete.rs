use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Widget};

use crate::components::theme;
use crate::components::{Component, KeyEventResponse, fit_tail};
use crate::remove::{Scope, Target};

/// One marked worktree, as the confirmation screen shows it.
pub struct Entry {
    pub label: String,
    pub branch: Option<String>,
    /// The remote holding a copy of this branch, if one was ever pushed.
    pub remote: Option<String>,
    pub path: PathBuf,
    pub dirty: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Cancel,
    Delete { force: bool },
}

pub struct ConfirmDelete {
    entries: Vec<Entry>,
    scope: Scope,
    outcome: Option<Outcome>,
}

impl ConfirmDelete {
    pub fn new(entries: Vec<Entry>) -> Self {
        Self {
            entries,
            scope: Scope::default(),
            outcome: None,
        }
    }

    /// How far the deletion reaches; the caller adds `force`.
    pub fn scope(&self) -> Scope {
        self.scope
    }

    pub fn take_outcome(&mut self) -> Option<Outcome> {
        self.outcome.take()
    }

    pub fn targets(&self) -> Vec<Target> {
        self.entries
            .iter()
            .map(|entry| Target {
                label: entry.label.clone(),
                branch: entry.branch.clone(),
                remote: entry.remote.clone(),
                path: entry.path.clone(),
            })
            .collect()
    }

    fn any_dirty(&self) -> bool {
        self.entries.iter().any(|entry| entry.dirty)
    }

    fn any_branch(&self) -> bool {
        self.entries.iter().any(|entry| entry.branch.is_some())
    }

    fn any_remote(&self) -> bool {
        self.entries.iter().any(|entry| entry.remote.is_some())
    }

    fn title(&self) -> String {
        let count = self.entries.len();
        let noun = if count == 1 { "worktree" } else { "worktrees" };
        format!(" {} delete {count} {noun} ", theme::TRASH)
    }
}

fn checkbox(checked: bool) -> &'static str {
    if checked {
        theme::CHECKED
    } else {
        theme::UNCHECKED
    }
}

impl Component for ConfirmDelete {
    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let [body, footer] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

        let block = Block::bordered().title(Span::styled(self.title(), theme::danger()));
        let inner = block.inner(body);
        block.render(body, frame.buffer_mut());

        let room_for_branch = (inner.width.saturating_sub(20) as usize).max(12);
        let label_width = self
            .entries
            .iter()
            .map(|entry| entry.label.chars().count())
            .max()
            .unwrap_or(0)
            .clamp(12, room_for_branch);

        let mut lines: Vec<Line> = self
            .entries
            .iter()
            .map(|entry| {
                let remote = match entry.remote.as_deref() {
                    Some(remote) => format!("  ({remote})"),
                    None => String::new(),
                };
                let mut spans = vec![Span::from(format!(
                    " {:<label_width$}  {} {}{remote}  ",
                    fit_tail(&entry.label, label_width),
                    theme::BRANCH,
                    entry.branch.as_deref().unwrap_or("(detached)"),
                ))];
                if entry.dirty {
                    spans.push(Span::styled(theme::DIRTY, theme::dirty()));
                }
                Line::from(spans)
            })
            .collect();

        lines.push(Line::default());
        if self.any_branch() {
            lines.push(Line::from(format!(
                " {} delete branch too",
                checkbox(self.scope.branch)
            )));
        }
        if self.any_remote() {
            lines.push(Line::from(format!(
                " {} delete the remote branch too",
                checkbox(self.scope.remote)
            )));
        }
        if self.scope.remote {
            lines.push(Line::styled(
                format!(
                    " {} deleting a remote branch is visible to everyone on the repo",
                    theme::WARN
                ),
                theme::danger(),
            ));
        }
        if self.any_dirty() {
            lines.push(
                Line::from(format!(
                    " {} has uncommitted changes — enter refuses it, f forces it",
                    theme::DIRTY
                ))
                .dim(),
            );
        }

        Paragraph::new(lines).render(inner, frame.buffer_mut());

        let branch_hint = if self.any_branch() {
            "Branch: <space> | "
        } else {
            ""
        };
        let remote_hint = if self.any_remote() {
            "Remote: r | "
        } else {
            ""
        };
        Paragraph::new(
            Line::from(format!(
                " {branch_hint}{remote_hint}Delete: <enter> | Force: f | Cancel: <esc>"
            ))
            .dim(),
        )
        .render(footer, frame.buffer_mut());
    }

    fn handle_event_key(&mut self, key_event: KeyEvent) -> KeyEventResponse {
        if key_event.kind != KeyEventKind::Press {
            return KeyEventResponse::Ignored;
        }
        match key_event.code {
            KeyCode::Char(' ') if self.any_branch() => self.scope.branch = !self.scope.branch,
            KeyCode::Char('r') if self.any_remote() => self.scope.remote = !self.scope.remote,
            KeyCode::Enter => self.outcome = Some(Outcome::Delete { force: false }),
            KeyCode::Char('f') => self.outcome = Some(Outcome::Delete { force: true }),
            KeyCode::Esc => self.outcome = Some(Outcome::Cancel),
            _ => return KeyEventResponse::Ignored,
        }
        KeyEventResponse::Consumed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{buffer_to_string, key};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn entry(label: &str, branch: Option<&str>, dirty: bool) -> Entry {
        Entry {
            label: String::from(label),
            branch: branch.map(String::from),
            remote: None,
            path: PathBuf::from(format!("/w/{label}")),
            dirty,
        }
    }

    fn tracked(label: &str, branch: &str) -> Entry {
        Entry {
            remote: Some(String::from("gitlab")),
            ..entry(label, Some(branch), false)
        }
    }

    fn component() -> ConfirmDelete {
        ConfirmDelete::new(vec![
            entry("one", Some("feature/one"), false),
            entry("two", Some("two"), true),
        ])
    }

    fn dump(component: &mut ConfirmDelete) -> String {
        let mut terminal = Terminal::new(TestBackend::new(70, 10)).unwrap();
        terminal
            .draw(|frame| component.render(frame, frame.area()))
            .unwrap();
        buffer_to_string(terminal.backend().buffer())
    }

    #[test]
    fn test_new_keeps_branches_until_the_toggle_is_flipped() {
        assert!(!component().scope().branch);
    }

    #[test]
    fn test_space_toggles_branch_deletion() {
        let mut component = component();
        component.handle_event_key(key(KeyCode::Char(' ')));
        assert!(component.scope().branch);
        component.handle_event_key(key(KeyCode::Char(' ')));
        assert!(!component.scope().branch);
    }

    #[test]
    fn test_enter_asks_for_a_plain_delete() {
        let mut component = component();
        component.handle_event_key(key(KeyCode::Enter));
        assert_eq!(
            component.take_outcome(),
            Some(Outcome::Delete { force: false })
        );
    }

    #[test]
    fn test_f_asks_for_a_forced_delete() {
        let mut component = component();
        component.handle_event_key(key(KeyCode::Char('f')));
        assert_eq!(
            component.take_outcome(),
            Some(Outcome::Delete { force: true })
        );
    }

    #[test]
    fn test_esc_cancels() {
        let mut component = component();
        component.handle_event_key(key(KeyCode::Esc));
        assert_eq!(component.take_outcome(), Some(Outcome::Cancel));
    }

    #[test]
    fn test_an_outcome_is_only_reported_once() {
        let mut component = component();
        component.handle_event_key(key(KeyCode::Enter));
        assert!(component.take_outcome().is_some());
        assert_eq!(component.take_outcome(), None);
    }

    #[test]
    fn test_no_key_yet_means_no_outcome() {
        assert_eq!(component().take_outcome(), None);
    }

    #[test]
    fn test_targets_carry_the_paths_and_branches_of_the_marked_rows() {
        let component = component();
        assert_eq!(
            component.targets(),
            vec![
                crate::remove::Target {
                    label: String::from("one"),
                    branch: Some(String::from("feature/one")),
                    remote: None,
                    path: PathBuf::from("/w/one"),
                },
                crate::remove::Target {
                    label: String::from("two"),
                    branch: Some(String::from("two")),
                    remote: None,
                    path: PathBuf::from("/w/two"),
                },
            ]
        );
    }

    #[test]
    fn test_render_lists_every_worktree_with_its_branch() {
        let text = dump(&mut component());
        assert!(text.contains("one"), "{text}");
        assert!(text.contains("feature/one"), "{text}");
        assert!(text.contains("two"), "{text}");
    }

    #[test]
    fn test_render_counts_the_worktrees_in_the_title() {
        let text = dump(&mut component());
        assert!(text.contains("delete 2 worktrees"), "{text}");
    }

    #[test]
    fn test_render_says_one_worktree_in_the_singular() {
        let mut component = ConfirmDelete::new(vec![entry("one", None, false)]);
        let text = dump(&mut component);
        assert!(text.contains("delete 1 worktree "), "{text}");
    }

    #[test]
    fn test_render_marks_a_worktree_with_uncommitted_changes() {
        let text = dump(&mut component());
        assert!(text.contains('●'), "{text}");
    }

    #[test]
    fn test_render_warns_that_a_plain_delete_will_refuse_the_dirty_ones() {
        let text = dump(&mut component());
        assert!(text.contains("uncommitted"), "{text}");
    }

    #[test]
    fn test_render_keeps_quiet_when_nothing_is_dirty() {
        let mut component = ConfirmDelete::new(vec![entry("one", None, false)]);
        let text = dump(&mut component);
        assert!(!text.contains("uncommitted"), "{text}");
        assert!(!text.contains('●'), "{text}");
    }

    #[test]
    fn test_render_shows_the_branch_toggle_unchecked() {
        let text = dump(&mut component());
        assert!(
            text.contains(&format!("{} delete branch too", theme::UNCHECKED)),
            "{text}"
        );
    }

    #[test]
    fn test_render_shows_the_branch_toggle_checked_once_flipped() {
        let mut component = component();
        component.handle_event_key(key(KeyCode::Char(' ')));
        let text = dump(&mut component);
        assert!(
            text.contains(&format!("{} delete branch too", theme::CHECKED)),
            "{text}"
        );
    }

    #[test]
    fn test_render_hides_the_branch_toggle_when_nothing_has_a_branch() {
        let mut component = ConfirmDelete::new(vec![entry("one", None, false)]);
        let text = dump(&mut component);
        assert!(!text.contains("delete branch too"), "{text}");
    }

    #[test]
    fn test_new_keeps_the_remote_branch_until_the_toggle_is_flipped() {
        let component = ConfirmDelete::new(vec![tracked("one", "one")]);
        assert!(!component.scope().remote);
    }

    #[test]
    fn test_r_toggles_remote_branch_deletion() {
        let mut component = ConfirmDelete::new(vec![tracked("one", "one")]);
        component.handle_event_key(key(KeyCode::Char('r')));
        assert!(component.scope().remote);
        component.handle_event_key(key(KeyCode::Char('r')));
        assert!(!component.scope().remote);
    }

    #[test]
    fn test_the_remote_toggle_is_independent_of_the_local_one() {
        let mut component = ConfirmDelete::new(vec![tracked("one", "one")]);
        component.handle_event_key(key(KeyCode::Char('r')));
        assert!(component.scope().remote);
        assert!(!component.scope().branch, "the local branch can stay");
    }

    #[test]
    fn test_r_does_nothing_when_no_branch_was_ever_pushed() {
        let mut component = component();
        component.handle_event_key(key(KeyCode::Char('r')));
        assert!(!component.scope().remote);
    }

    #[test]
    fn test_targets_carry_the_remote_of_a_pushed_branch() {
        let component = ConfirmDelete::new(vec![tracked("one", "one")]);
        assert_eq!(component.targets()[0].remote.as_deref(), Some("gitlab"));
    }

    #[test]
    fn test_render_shows_the_remote_toggle_unchecked() {
        let mut component = ConfirmDelete::new(vec![tracked("one", "one")]);
        let text = dump(&mut component);
        assert!(
            text.contains(&format!(
                "{} delete the remote branch too",
                theme::UNCHECKED
            )),
            "{text}"
        );
    }

    #[test]
    fn test_render_shows_the_remote_toggle_checked_once_flipped() {
        let mut component = ConfirmDelete::new(vec![tracked("one", "one")]);
        component.handle_event_key(key(KeyCode::Char('r')));
        let text = dump(&mut component);
        assert!(
            text.contains(&format!("{} delete the remote branch too", theme::CHECKED)),
            "{text}"
        );
    }

    #[test]
    fn test_render_hides_the_remote_toggle_when_nothing_was_pushed() {
        let text = dump(&mut component());
        assert!(!text.contains("remote branch"), "{text}");
    }

    #[test]
    fn test_render_warns_that_a_remote_deletion_is_not_yours_alone() {
        let mut component = ConfirmDelete::new(vec![tracked("one", "one")]);
        component.handle_event_key(key(KeyCode::Char('r')));
        let text = dump(&mut component);
        assert!(text.contains("everyone"), "{text}");
    }

    #[test]
    fn test_render_keeps_quiet_about_the_remote_until_the_toggle_is_on() {
        let mut component = ConfirmDelete::new(vec![tracked("one", "one")]);
        let text = dump(&mut component);
        assert!(!text.contains("everyone"), "{text}");
    }

    #[test]
    fn test_render_names_the_remote_on_the_row_that_has_one() {
        let mut component = ConfirmDelete::new(vec![tracked("one", "one")]);
        let text = dump(&mut component);
        assert!(text.contains("gitlab"), "{text}");
    }

    #[test]
    fn test_render_shows_the_keys() {
        let text = dump(&mut component());
        assert!(text.contains("Delete: <enter>"), "{text}");
        assert!(!text.contains("Remote: r"), "nothing was pushed: {text}");
        assert!(text.contains("Force: f"), "{text}");
        assert!(text.contains("Cancel: <esc>"), "{text}");
    }

    #[test]
    fn test_render_offers_the_remote_key_when_something_was_pushed() {
        let mut component = ConfirmDelete::new(vec![tracked("one", "one")]);
        let text = dump(&mut component);
        assert!(text.contains("Remote: r"), "{text}");
    }

    #[test]
    fn test_render_survives_a_narrow_terminal() {
        let mut component = component();
        let mut terminal = Terminal::new(TestBackend::new(20, 6)).unwrap();
        terminal
            .draw(|frame| component.render(frame, frame.area()))
            .unwrap();
    }
}
