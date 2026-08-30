use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Widget};

use crate::components::theme;
use crate::components::{Component, KeyEventResponse, fit_tail};

/// What a producer of steps reports back to the screen.
pub enum Progress {
    Running(usize),
    Ok(usize, String),
    Failed(usize, String),
    Finished,
}

/// What the screen offers once every step has settled.
#[derive(Clone, Copy)]
pub enum Completion {
    /// A run that leaves a directory worth landing in.
    CdOrList { shell_init: bool },
    /// A run with nowhere to go but back.
    ListOnly,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StepState {
    Pending,
    Running,
    Done,
    Failed,
}

pub struct ProgressComponent {
    title: String,
    labels: Vec<String>,
    states: Vec<StepState>,
    details: Vec<String>,
    finished: bool,
    failure: Option<String>,
    completion: Completion,
    frame: usize,
}

impl ProgressComponent {
    pub fn new(title: String, labels: Vec<String>, completion: Completion) -> Self {
        let count = labels.len();
        Self {
            title,
            labels,
            states: vec![StepState::Pending; count],
            details: vec![String::new(); count],
            finished: false,
            failure: None,
            completion,
            frame: 0,
        }
    }

    pub fn tick(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }

    fn hints(&self) -> &'static str {
        if !self.finished {
            return "";
        }
        match self.completion {
            _ if self.failure.is_some() => " List: <enter>",
            Completion::ListOnly => " List: <enter>",
            Completion::CdOrList { shell_init: true } => " Cd: <enter> | List: <esc>",
            Completion::CdOrList { shell_init: false } => " Cd: … (needs shell init) | List: <esc>",
        }
    }

    pub fn finished(&self) -> bool {
        self.finished
    }

    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    pub fn apply(&mut self, progress: Progress) {
        match progress {
            Progress::Running(index) => self.set_state(index, StepState::Running),
            Progress::Ok(index, detail) => {
                self.set_state(index, StepState::Done);
                if let Some(slot) = self.details.get_mut(index) {
                    *slot = detail;
                }
            }
            Progress::Failed(index, message) => {
                self.set_state(index, StepState::Failed);
                if let Some(slot) = self.details.get_mut(index) {
                    *slot = message.clone();
                }
                self.failure = Some(message);
            }
            Progress::Finished => self.finished = true,
        }
    }

    fn set_state(&mut self, index: usize, state: StepState) {
        if let Some(slot) = self.states.get_mut(index) {
            *slot = state;
        }
    }
}

impl Component for ProgressComponent {
    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let [body, footer] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

        let block = Block::bordered().title(Line::from(vec![
            Span::from(" "),
            Span::styled(self.title.as_str(), theme::title()),
            Span::from(" "),
        ]));
        let inner = block.inner(body);
        block.render(body, frame.buffer_mut());

        let room_for_detail = (inner.width.saturating_sub(24) as usize).max(12);
        let label_width = self
            .labels
            .iter()
            .map(|label| label.chars().count())
            .max()
            .unwrap_or(0)
            .clamp(12, room_for_detail);

        let lines: Vec<Line> = self
            .labels
            .iter()
            .enumerate()
            .map(|(index, label)| {
                let spinner = theme::spinner()[self.frame % theme::spinner().len()];
                let (marker, style) = match self.states.get(index) {
                    Some(StepState::Done) => (String::from(theme::done()), theme::ok()),
                    Some(StepState::Failed) => (String::from(theme::failed()), theme::danger()),
                    Some(StepState::Running) => (spinner.to_string(), theme::accent()),
                    _ => (String::from(" "), Style::new()),
                };
                let detail = self.details.get(index).map(String::as_str).unwrap_or("");
                Line::from(vec![
                    Span::styled(format!("{marker} "), style),
                    Span::from(format!(
                        "{:<label_width$} {detail}",
                        fit_tail(label, label_width)
                    )),
                ])
            })
            .collect();

        Paragraph::new(lines).render(inner, frame.buffer_mut());
        Paragraph::new(Line::from(self.hints()).dim()).render(footer, frame.buffer_mut());
    }

    fn handle_event_key(&mut self, _key_event: crossterm::event::KeyEvent) -> KeyEventResponse {
        KeyEventResponse::Ignored
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::buffer_to_string;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn component() -> ProgressComponent {
        component_with_shell_init(true)
    }

    fn component_with_shell_init(shell_init: bool) -> ProgressComponent {
        let completion = Completion::CdOrList { shell_init };
        ProgressComponent::new(
            String::from("creating spectra-spe-11667"),
            vec![
                String::from("git worktree add  feature/spe-11667"),
                String::from("npm ci  app"),
            ],
            completion,
        )
    }

    fn dump(component: &mut ProgressComponent) -> String {
        let mut terminal = Terminal::new(TestBackend::new(70, 8)).unwrap();
        terminal
            .draw(|frame| component.render(frame, frame.area()))
            .unwrap();
        buffer_to_string(terminal.backend().buffer())
    }

    #[test]
    fn test_new_is_not_finished() {
        let component = component();
        assert!(!component.finished());
        assert_eq!(component.failure(), None);
    }

    #[test]
    fn test_apply_finished_marks_finished() {
        let mut component = component();
        component.apply(Progress::Finished);
        assert!(component.finished());
    }

    #[test]
    fn test_apply_failed_records_the_failure_without_finishing() {
        let mut component = component();
        component.apply(Progress::Failed(1, String::from("cp failed")));
        assert_eq!(component.failure(), Some("cp failed"));
        assert!(!component.finished());
    }

    #[test]
    fn test_apply_finished_after_a_failure_finishes() {
        let mut component = component();
        component.apply(Progress::Failed(1, String::from("cp failed")));
        component.apply(Progress::Finished);
        assert!(component.finished());
        assert_eq!(component.failure(), Some("cp failed"));
    }

    /// The marker cell of a step: column 1, row 1, both inside the block border.
    fn marker(component: &mut ProgressComponent, step: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(70, 8)).unwrap();
        terminal
            .draw(|frame| component.render(frame, frame.area()))
            .unwrap();
        terminal.backend().buffer()[(1, 1 + step)]
            .symbol()
            .to_string()
    }

    #[test]
    fn test_running_marker_advances_on_each_tick() {
        let mut component = component();
        component.apply(Progress::Running(1));
        let first = marker(&mut component, 1);
        component.tick();
        let second = marker(&mut component, 1);
        assert_ne!(first, second);
    }

    #[test]
    fn test_running_marker_wraps_back_to_the_first_frame() {
        let mut component = component();
        component.apply(Progress::Running(1));
        let first = marker(&mut component, 1);
        for _ in 0..theme::spinner().len() {
            component.tick();
        }
        assert_eq!(marker(&mut component, 1), first);
    }

    #[test]
    fn test_tick_leaves_a_settled_marker_alone() {
        let mut component = component();
        component.apply(Progress::Ok(1, String::from("installed")));
        component.tick();
        assert_eq!(marker(&mut component, 1), theme::done());
    }

    #[test]
    fn test_render_shows_step_labels_and_detail() {
        let mut component = component();
        component.apply(Progress::Ok(0, String::from("created")));
        component.apply(Progress::Running(1));
        let text = dump(&mut component);
        assert!(text.contains("feature/spe-11667"), "{text}");
        assert!(text.contains("created"), "{text}");
        assert!(text.contains("npm ci  app"), "{text}");
        assert!(text.contains(theme::done()), "{text}");
    }

    #[test]
    fn test_render_keeps_the_detail_column_aligned_when_labels_are_long() {
        let mut component = ProgressComponent::new(
            String::from("creating spectra-fissa-smoke-test"),
            vec![
                String::from("git worktree add  feature/fissa-smoke-test"),
                String::from("npm ci  app"),
            ],
            Completion::CdOrList { shell_init: true },
        );
        component.apply(Progress::Ok(0, String::from("DETAIL-A")));
        component.apply(Progress::Ok(1, String::from("DETAIL-B")));
        let text = dump(&mut component);
        let lines: Vec<String> = text
            .chars()
            .collect::<Vec<_>>()
            .chunks(70)
            .map(|chunk| chunk.iter().collect())
            .collect();

        let column_of = |needle: &str| -> Option<usize> {
            let line = lines.iter().find(|l| l.contains(needle))?;
            let byte = line.find(needle)?;
            Some(line[..byte].chars().count())
        };
        assert_eq!(
            column_of("DETAIL-A"),
            column_of("DETAIL-B"),
            "detail column misaligned:\n{text}"
        );
    }

    #[test]
    fn test_render_survives_a_terminal_too_narrow_for_the_detail_column() {
        let mut component = component();
        component.apply(Progress::Ok(0, String::from("created")));
        let mut terminal = Terminal::new(TestBackend::new(20, 6)).unwrap();
        terminal
            .draw(|frame| component.render(frame, frame.area()))
            .unwrap();
    }

    #[test]
    fn test_render_shows_no_hints_while_the_run_is_unfinished() {
        let mut component = component();
        component.apply(Progress::Running(0));
        let text = dump(&mut component);
        assert!(!text.contains("enter"), "{text}");
    }

    #[test]
    fn test_render_offers_cd_when_a_run_finishes_with_the_wrapper_active() {
        let mut component = component();
        component.apply(Progress::Finished);
        let text = dump(&mut component);
        assert!(text.contains("Cd: <enter>"), "{text}");
        assert!(text.contains("List: <esc>"), "{text}");
    }

    #[test]
    fn test_render_reminds_about_shell_init_when_the_wrapper_is_inactive() {
        let mut component = component_with_shell_init(false);
        component.apply(Progress::Finished);
        let text = dump(&mut component);
        assert!(text.contains("needs shell init"), "{text}");
        assert!(!text.contains("Cd: <enter>"), "{text}");
    }

    #[test]
    fn test_render_offers_only_the_list_when_there_is_nothing_to_cd_into() {
        let mut component = ProgressComponent::new(
            String::from("deleting 2 worktrees"),
            vec![String::from("git worktree remove  side")],
            Completion::ListOnly,
        );
        component.apply(Progress::Finished);
        let text = dump(&mut component);
        assert!(text.contains("List: <enter>"), "{text}");
        assert!(!text.contains("Cd: <enter>"), "{text}");
    }

    #[test]
    fn test_render_offers_only_the_list_after_a_failure() {
        let mut component = component();
        component.apply(Progress::Failed(0, String::from("branch exists")));
        component.apply(Progress::Finished);
        let text = dump(&mut component);
        assert!(text.contains("List: <enter>"), "{text}");
        assert!(!text.contains("Cd: <enter>"), "{text}");
    }

    #[test]
    fn test_render_shows_the_failure_message() {
        let mut component = component();
        component.apply(Progress::Failed(0, String::from("branch exists")));
        let text = dump(&mut component);
        assert!(text.contains("branch exists"), "{text}");
        assert!(text.contains(theme::failed()), "{text}");
    }
}
