use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Widget};

use crate::components::{Component, KeyEventResponse};
use crate::naming;
use crate::node::Strategy;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Slug,
    Prefix,
    Base,
    Deps,
}

pub struct CreateForm {
    repo_dir: String,
    slug: String,
    prefixes: Vec<String>,
    prefix_index: usize,
    base: String,
    allowed: Vec<Strategy>,
    strategy_index: usize,
    fields: Vec<Field>,
    focus: Field,
    error: Option<String>,
    submit: bool,
    cancel: bool,
}

impl CreateForm {
    pub fn new(
        repo_dir: String,
        prefixes: Vec<String>,
        base: String,
        allowed: Vec<Strategy>,
    ) -> Self {
        let mut fields = vec![Field::Slug, Field::Prefix, Field::Base];
        if allowed.contains(&Strategy::Install) {
            fields.push(Field::Deps);
        }
        Self {
            repo_dir,
            slug: String::new(),
            prefixes: if prefixes.is_empty() {
                vec![String::new()]
            } else {
                prefixes
            },
            prefix_index: 0,
            base,
            allowed,
            strategy_index: 0,
            fields,
            focus: Field::Slug,
            error: None,
            submit: false,
            cancel: false,
        }
    }

    #[allow(dead_code)] // read only by tests; the rendered marker uses self.focus directly
    pub fn focus(&self) -> Field {
        self.focus
    }

    pub fn shows_deps(&self) -> bool {
        self.allowed.contains(&Strategy::Install)
    }

    pub fn prefix(&self) -> &str {
        self.prefixes
            .get(self.prefix_index)
            .map(String::as_str)
            .unwrap_or("")
    }

    pub fn prefix_overridden(&self) -> bool {
        self.slug.contains('/')
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn strategy(&self) -> Strategy {
        self.allowed
            .get(self.strategy_index)
            .copied()
            .unwrap_or(Strategy::None)
    }

    pub fn set_error(&mut self, error: Option<String>) {
        self.error = error;
    }

    pub fn take_submit(&mut self) -> bool {
        std::mem::take(&mut self.submit)
    }

    pub fn take_cancel(&mut self) -> bool {
        std::mem::take(&mut self.cancel)
    }

    fn names(&self) -> Option<naming::Names> {
        naming::derive_names(&self.slug, self.prefix(), &self.repo_dir)
    }

    pub fn branch(&self) -> Option<String> {
        self.names().map(|n| n.branch)
    }

    pub fn dir(&self) -> Option<String> {
        self.names().map(|n| n.dir)
    }

    fn cycle(index: usize, len: usize, delta: isize) -> usize {
        if len == 0 {
            return 0;
        }
        let len = len as isize;
        (((index as isize + delta) % len + len) % len) as usize
    }

    fn move_focus(&mut self, delta: isize) {
        let current = self
            .fields
            .iter()
            .position(|field| *field == self.focus)
            .unwrap_or(0);
        let next = Self::cycle(current, self.fields.len(), delta);
        self.focus = self.fields.get(next).copied().unwrap_or(Field::Slug);
    }

    fn cycle_focused(&mut self, delta: isize) {
        match self.focus {
            Field::Prefix => {
                self.prefix_index = Self::cycle(self.prefix_index, self.prefixes.len(), delta);
            }
            Field::Deps => {
                self.strategy_index = Self::cycle(self.strategy_index, self.allowed.len(), delta);
            }
            _ => {}
        }
    }

    fn push_char(&mut self, ch: char) {
        match self.focus {
            Field::Slug => self.slug.push(ch),
            Field::Base => self.base.push(ch),
            _ => {}
        }
        self.error = None;
    }

    fn pop_char(&mut self) {
        match self.focus {
            Field::Slug => {
                self.slug.pop();
            }
            Field::Base => {
                self.base.pop();
            }
            _ => {}
        }
        self.error = None;
    }
}

impl Component for CreateForm {
    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let block = Block::bordered().title(" new worktree ");
        let inner = block.inner(area);
        block.render(area, frame.buffer_mut());

        let [fields, preview, error, footer] = Layout::vertical([
            Constraint::Length(self.fields.len() as u16),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);

        let marker = |field: Field| if self.focus == field { ">" } else { " " };
        let prefix_display = if self.prefix_overridden() {
            format!("‹ {} ›  (overridden)", self.prefix())
        } else {
            format!("‹ {} ›", self.prefix())
        };

        let mut lines = vec![
            format!("{} slug     {}", marker(Field::Slug), self.slug),
            format!("{} prefix   {}", marker(Field::Prefix), prefix_display),
            format!("{} base     {}", marker(Field::Base), self.base),
        ];
        if self.shows_deps() {
            lines.push(format!(
                "{} deps     ‹ {} ›",
                marker(Field::Deps),
                self.strategy().label()
            ));
        }

        // Only the typed fields get a cursor; prefix and deps are cycled with ←/→.
        if matches!(self.focus, Field::Slug | Field::Base)
            && let Some(row) = self.fields.iter().position(|field| *field == self.focus)
            && let Some(line) = lines.get(row)
        {
            let x = fields.x.saturating_add(line.chars().count() as u16);
            frame.set_cursor_position((
                x.min(fields.right().saturating_sub(1)),
                fields.y.saturating_add(row as u16),
            ));
        }

        let lines: Vec<Line> = lines.into_iter().map(Line::from).collect();
        Paragraph::new(lines).render(fields, frame.buffer_mut());

        let branch = self.branch().unwrap_or_else(|| String::from("—"));
        let dir = self.dir().unwrap_or_else(|| String::from("—"));
        Paragraph::new(vec![
            Line::from(""),
            Line::from(format!("  branch   {branch}")),
            Line::from(format!("  dir      {dir}")),
        ])
        .render(preview, frame.buffer_mut());

        if let Some(message) = &self.error {
            Paragraph::new(Line::from(vec![
                Span::from("  ! "),
                Span::from(message.as_str()),
            ]))
            .render(error, frame.buffer_mut());
        }

        Paragraph::new(Line::from(" tab/↑↓ field   ←→ cycle   enter create   esc cancel").dim())
            .render(footer, frame.buffer_mut());
    }

    fn handle_event_key(&mut self, key_event: KeyEvent) -> KeyEventResponse {
        if key_event.kind != KeyEventKind::Press {
            return KeyEventResponse::Ignored;
        }
        match key_event.code {
            KeyCode::Tab | KeyCode::Down => {
                self.move_focus(1);
                KeyEventResponse::Consumed
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.move_focus(-1);
                KeyEventResponse::Consumed
            }
            KeyCode::Right => {
                self.cycle_focused(1);
                KeyEventResponse::Consumed
            }
            KeyCode::Left => {
                self.cycle_focused(-1);
                KeyEventResponse::Consumed
            }
            KeyCode::Backspace => {
                self.pop_char();
                KeyEventResponse::Consumed
            }
            KeyCode::Char(ch) => {
                self.push_char(ch);
                KeyEventResponse::Consumed
            }
            KeyCode::Enter => {
                if self.branch().is_some() {
                    self.submit = true;
                }
                KeyEventResponse::Consumed
            }
            KeyCode::Esc => {
                self.cancel = true;
                KeyEventResponse::Consumed
            }
            _ => KeyEventResponse::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{buffer_to_string, key};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Position;

    fn form() -> CreateForm {
        CreateForm::new(
            String::from("spectra"),
            vec![
                String::from("feature/"),
                String::from("fix/"),
                String::new(),
            ],
            String::from("develop"),
            vec![Strategy::Install, Strategy::None],
        )
    }

    fn form_without_deps() -> CreateForm {
        CreateForm::new(
            String::from("spectra"),
            vec![String::from("feature/")],
            String::from("develop"),
            vec![],
        )
    }

    fn type_str(form: &mut CreateForm, text: &str) {
        for ch in text.chars() {
            form.handle_event_key(key(KeyCode::Char(ch)));
        }
    }

    fn render_to_terminal(form: &mut CreateForm) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(70, 12)).unwrap();
        terminal
            .draw(|frame| form.render(frame, frame.area()))
            .unwrap();
        terminal
    }

    fn dump(form: &mut CreateForm) -> String {
        buffer_to_string(render_to_terminal(form).backend().buffer())
    }

    #[test]
    fn test_empty_slug_has_no_branch_or_dir() {
        let form = form();
        assert_eq!(form.branch(), None);
        assert_eq!(form.dir(), None);
    }

    #[test]
    fn test_typing_a_slug_previews_branch_and_dir() {
        let mut form = form();
        type_str(&mut form, "spe-11667");
        assert_eq!(form.branch().as_deref(), Some("feature/spe-11667"));
        assert_eq!(form.dir().as_deref(), Some("spectra-spe-11667"));
    }

    #[test]
    fn test_backspace_removes_the_last_slug_character() {
        let mut form = form();
        type_str(&mut form, "abc");
        form.handle_event_key(key(KeyCode::Backspace));
        assert_eq!(form.branch().as_deref(), Some("feature/ab"));
    }

    #[test]
    fn test_slug_with_slash_overrides_the_prefix() {
        let mut form = form();
        type_str(&mut form, "fix/spe-1");
        assert!(form.prefix_overridden());
        assert_eq!(form.branch().as_deref(), Some("fix/spe-1"));
    }

    #[test]
    fn test_prefix_not_overridden_without_a_slash() {
        let mut form = form();
        type_str(&mut form, "spe-1");
        assert!(!form.prefix_overridden());
    }

    #[test]
    fn test_tab_cycles_focus_through_all_fields() {
        let mut form = form();
        assert_eq!(form.focus(), Field::Slug);
        form.handle_event_key(key(KeyCode::Tab));
        assert_eq!(form.focus(), Field::Prefix);
        form.handle_event_key(key(KeyCode::Tab));
        assert_eq!(form.focus(), Field::Base);
        form.handle_event_key(key(KeyCode::Tab));
        assert_eq!(form.focus(), Field::Deps);
        form.handle_event_key(key(KeyCode::Tab));
        assert_eq!(form.focus(), Field::Slug);
    }

    #[test]
    fn test_down_arrow_moves_to_the_next_field() {
        let mut form = form();
        assert!(matches!(
            form.handle_event_key(key(KeyCode::Down)),
            KeyEventResponse::Consumed
        ));
        assert_eq!(form.focus(), Field::Prefix);
    }

    #[test]
    fn test_up_arrow_moves_to_the_previous_field() {
        let mut form = form();
        form.handle_event_key(key(KeyCode::Down));
        assert!(matches!(
            form.handle_event_key(key(KeyCode::Up)),
            KeyEventResponse::Consumed
        ));
        assert_eq!(form.focus(), Field::Slug);
    }

    #[test]
    fn test_up_arrow_from_the_first_field_wraps_to_the_last() {
        let mut form = form();
        form.handle_event_key(key(KeyCode::Up));
        assert_eq!(form.focus(), Field::Deps);
    }

    #[test]
    fn test_shift_tab_moves_to_the_previous_field() {
        let mut form = form();
        form.handle_event_key(key(KeyCode::BackTab));
        assert_eq!(form.focus(), Field::Deps);
    }

    #[test]
    fn test_focus_skips_deps_when_no_strategies_are_available() {
        let mut form = form_without_deps();
        assert_eq!(form.focus(), Field::Slug);
        form.handle_event_key(key(KeyCode::Tab));
        assert_eq!(form.focus(), Field::Prefix);
        form.handle_event_key(key(KeyCode::Tab));
        assert_eq!(form.focus(), Field::Base);
        form.handle_event_key(key(KeyCode::Tab));
        assert_eq!(form.focus(), Field::Slug);
    }

    #[test]
    fn test_right_on_prefix_field_selects_the_next_prefix() {
        let mut form = form();
        type_str(&mut form, "spe-1");
        form.handle_event_key(key(KeyCode::Tab));
        form.handle_event_key(key(KeyCode::Right));
        assert_eq!(form.branch().as_deref(), Some("fix/spe-1"));
    }

    #[test]
    fn test_prefix_cycles_to_empty_giving_a_bare_branch() {
        let mut form = form();
        type_str(&mut form, "spe-1");
        form.handle_event_key(key(KeyCode::Tab));
        form.handle_event_key(key(KeyCode::Right));
        form.handle_event_key(key(KeyCode::Right));
        assert_eq!(form.branch().as_deref(), Some("spe-1"));
    }

    #[test]
    fn test_left_on_prefix_field_wraps_backwards() {
        let mut form = form();
        type_str(&mut form, "spe-1");
        form.handle_event_key(key(KeyCode::Tab));
        form.handle_event_key(key(KeyCode::Left));
        assert_eq!(form.branch().as_deref(), Some("spe-1"));
    }

    #[test]
    fn test_typing_on_base_field_edits_the_base() {
        let mut form = form();
        form.handle_event_key(key(KeyCode::Tab));
        form.handle_event_key(key(KeyCode::Tab));
        form.handle_event_key(key(KeyCode::Backspace));
        assert_eq!(form.base(), "develo");
    }

    #[test]
    fn test_right_on_the_deps_field_selects_the_next_strategy() {
        let mut form = form();
        assert_eq!(form.strategy(), Strategy::Install);
        for _ in 0..3 {
            form.handle_event_key(key(KeyCode::Tab));
        }
        form.handle_event_key(key(KeyCode::Right));
        assert_eq!(form.strategy(), Strategy::None);
    }

    #[test]
    fn test_strategy_is_none_when_no_strategies_are_available() {
        let form = form_without_deps();
        assert_eq!(form.strategy(), Strategy::None);
    }

    #[test]
    fn test_enter_with_a_slug_submits() {
        let mut form = form();
        type_str(&mut form, "spe-1");
        form.handle_event_key(key(KeyCode::Enter));
        assert!(form.take_submit());
        assert!(!form.take_submit());
    }

    #[test]
    fn test_enter_without_a_slug_does_not_submit() {
        let mut form = form();
        form.handle_event_key(key(KeyCode::Enter));
        assert!(!form.take_submit());
    }

    #[test]
    fn test_esc_cancels() {
        let mut form = form();
        form.handle_event_key(key(KeyCode::Esc));
        assert!(form.take_cancel());
    }

    #[test]
    fn test_render_shows_live_preview_and_error() {
        let mut form = form();
        type_str(&mut form, "spe-11667");
        form.set_error(Some(String::from("directory already exists")));
        let text = dump(&mut form);
        assert!(text.contains("feature/spe-11667"), "{text}");
        assert!(text.contains("spectra-spe-11667"), "{text}");
        assert!(text.contains("directory already exists"), "{text}");
        assert!(text.contains("npm ci"), "{text}");
    }

    #[test]
    fn test_render_omits_the_deps_row_when_no_strategies_are_available() {
        let mut form = form_without_deps();
        let text = dump(&mut form);
        assert!(!text.contains("deps"), "{text}");
        assert!(text.contains("slug"), "{text}");
        assert!(text.contains("base"), "{text}");
    }

    #[test]
    fn test_the_focused_slug_field_holds_the_terminal_cursor() {
        let mut form = form();
        type_str(&mut form, "spe-11667");
        let terminal = render_to_terminal(&mut form);
        assert!(terminal.backend().cursor_visible());
        // "> slug     spe-11667" is 20 cells, starting one cell inside the border.
        assert_eq!(
            terminal.backend().cursor_position(),
            Position::from((21, 1))
        );
    }

    #[test]
    fn test_an_empty_slug_field_still_holds_the_cursor() {
        let terminal = render_to_terminal(&mut form());
        assert!(terminal.backend().cursor_visible());
        assert_eq!(
            terminal.backend().cursor_position(),
            Position::from((12, 1))
        );
    }

    #[test]
    fn test_the_cursor_follows_focus_to_the_base_field() {
        let mut form = form();
        form.handle_event_key(key(KeyCode::Down));
        form.handle_event_key(key(KeyCode::Down));
        let terminal = render_to_terminal(&mut form);
        assert!(terminal.backend().cursor_visible());
        // "> base     develop" is 18 cells, on the third field row.
        assert_eq!(
            terminal.backend().cursor_position(),
            Position::from((19, 3))
        );
    }

    #[test]
    fn test_a_cycled_field_hides_the_cursor() {
        let mut form = form();
        form.handle_event_key(key(KeyCode::Down));
        let terminal = render_to_terminal(&mut form);
        assert!(
            !terminal.backend().cursor_visible(),
            "prefix takes no characters"
        );
    }

    #[test]
    fn test_the_deps_field_hides_the_cursor() {
        let mut form = form();
        for _ in 0..3 {
            form.handle_event_key(key(KeyCode::Down));
        }
        let terminal = render_to_terminal(&mut form);
        assert!(!terminal.backend().cursor_visible());
    }

    #[test]
    fn test_render_footer_shows_both_field_movement_forms() {
        let text = dump(&mut form());
        assert!(text.contains("tab/↑↓ field"), "{text}");
        assert!(text.contains("←→ cycle"), "{text}");
    }

    #[test]
    fn test_render_labels_the_deps_row_with_the_command() {
        let mut form = form();
        let text = dump(&mut form);
        assert!(text.contains("deps"), "{text}");
        assert!(text.contains("npm ci"), "{text}");
        assert!(!text.contains("modules"), "{text}");
    }

    #[test]
    fn test_render_omits_the_deps_row_when_install_is_unavailable() {
        let mut form = CreateForm::new(
            String::from("spectra"),
            vec![String::from("feature/")],
            String::from("develop"),
            vec![Strategy::None],
        );
        let text = dump(&mut form);
        assert!(!text.contains("deps"), "{text}");
        assert!(text.contains("slug"), "{text}");
    }

    #[test]
    fn test_focus_skips_deps_when_install_is_unavailable() {
        let mut form = CreateForm::new(
            String::from("spectra"),
            vec![String::from("feature/")],
            String::from("develop"),
            vec![Strategy::None],
        );
        assert_eq!(form.focus(), Field::Slug);
        form.handle_event_key(key(KeyCode::Tab));
        assert_eq!(form.focus(), Field::Prefix);
        form.handle_event_key(key(KeyCode::Tab));
        assert_eq!(form.focus(), Field::Base);
        form.handle_event_key(key(KeyCode::Tab));
        assert_eq!(form.focus(), Field::Slug);
    }
}
