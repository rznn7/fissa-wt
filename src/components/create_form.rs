use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Widget};

use crate::components::text_input::TextInput;
use crate::components::theme;
use crate::components::{Component, KeyEventResponse};
use crate::naming;
use crate::node::Strategy;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Slug,
    Prefix,
    Base,
    Deps,
    Submodules,
}

pub struct CreateForm {
    repo_dir: String,
    slug: TextInput,
    prefixes: Vec<String>,
    prefix_index: usize,
    base: TextInput,
    allowed: Vec<Strategy>,
    strategy_index: usize,
    submodules: bool,
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
        has_submodules: bool,
    ) -> Self {
        let mut fields = vec![Field::Slug, Field::Prefix, Field::Base];
        if allowed.contains(&Strategy::Install) {
            fields.push(Field::Deps);
        }
        if has_submodules {
            fields.push(Field::Submodules);
        }
        Self {
            repo_dir,
            slug: TextInput::new(String::new()),
            prefixes: if prefixes.is_empty() {
                vec![String::new()]
            } else {
                prefixes
            },
            prefix_index: 0,
            base: TextInput::new(base),
            allowed,
            strategy_index: 0,
            submodules: has_submodules,
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

    pub fn submodules(&self) -> bool {
        self.submodules
    }

    fn shows_submodules(&self) -> bool {
        self.fields.contains(&Field::Submodules)
    }

    pub fn prefix(&self) -> &str {
        self.prefixes
            .get(self.prefix_index)
            .map(String::as_str)
            .unwrap_or("")
    }

    pub fn prefix_overridden(&self) -> bool {
        self.slug.value().contains('/')
    }

    pub fn base(&self) -> &str {
        self.base.value()
    }

    /// The focused field when it is one that accepts typing.
    fn focused_text(&self) -> Option<&TextInput> {
        match self.focus {
            Field::Slug => Some(&self.slug),
            Field::Base => Some(&self.base),
            _ => None,
        }
    }

    fn focused_text_mut(&mut self) -> Option<&mut TextInput> {
        match self.focus {
            Field::Slug => Some(&mut self.slug),
            Field::Base => Some(&mut self.base),
            _ => None,
        }
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
        naming::derive_names(self.slug.value(), self.prefix(), &self.repo_dir)
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
            Field::Submodules => self.submodules = !self.submodules,
            _ => {}
        }
    }

    fn push_char(&mut self, ch: char) {
        if let Some(input) = self.focused_text_mut() {
            input.insert(ch);
        }
        self.error = None;
    }

    fn pop_char(&mut self) {
        if let Some(input) = self.focused_text_mut() {
            input.backspace();
        }
        self.error = None;
    }

    fn delete_char(&mut self) {
        if let Some(input) = self.focused_text_mut() {
            input.delete();
        }
        self.error = None;
    }

    /// `←`/`→` move the cursor on a typed field and cycle the value on the others.
    fn move_or_cycle(&mut self, delta: isize) {
        match self.focused_text_mut() {
            Some(input) if delta < 0 => input.left(),
            Some(input) => input.right(),
            None => self.cycle_focused(delta),
        }
    }

    fn handle_control_key(&mut self, code: KeyCode) -> KeyEventResponse {
        let Some(input) = self.focused_text_mut() else {
            return KeyEventResponse::Ignored;
        };
        match code {
            KeyCode::Char('a') => input.home(),
            KeyCode::Char('e') => input.end(),
            KeyCode::Char('w') => {
                input.delete_word_back();
                self.error = None;
            }
            _ => return KeyEventResponse::Ignored,
        }
        KeyEventResponse::Consumed
    }
}

impl Component for CreateForm {
    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let [body, footer] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

        let block = Block::bordered().title(Line::from(vec![
            Span::from(format!(" {} ", theme::new())),
            Span::styled("new worktree", theme::title()),
            Span::from(" "),
        ]));
        let inner = block.inner(body);
        block.render(body, frame.buffer_mut());

        let [fields, preview, error] = Layout::vertical([
            Constraint::Length(self.fields.len() as u16),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .areas(inner);

        let marker = |field: Field| {
            if self.focus == field {
                theme::focus()
            } else {
                " "
            }
        };
        let prefix_display = if self.prefix_overridden() {
            format!("‹ {} ›  (overridden)", self.prefix())
        } else {
            format!("‹ {} ›", self.prefix())
        };

        let mut rows = vec![
            (
                format!("{} slug       ", marker(Field::Slug)),
                self.slug.value().to_string(),
            ),
            (
                format!("{} prefix     ", marker(Field::Prefix)),
                prefix_display,
            ),
            (
                format!("{} base       ", marker(Field::Base)),
                self.base.value().to_string(),
            ),
        ];
        if self.shows_deps() {
            rows.push((
                format!("{} deps       ", marker(Field::Deps)),
                format!("‹ {} ›", self.strategy().label()),
            ));
        }
        if self.shows_submodules() {
            let label = if self.submodules { "init" } else { "skip" };
            rows.push((
                format!("{} submodules ", marker(Field::Submodules)),
                format!("‹ {label} ›"),
            ));
        }

        // Only the typed fields get a cursor; the rest are cycled with ←/→.
        if let Some(cursor) = self.focused_text().map(TextInput::cursor)
            && let Some(row) = self.fields.iter().position(|field| *field == self.focus)
            && let Some((prefix, _)) = rows.get(row)
        {
            let offset = prefix.chars().count().saturating_add(cursor) as u16;
            frame.set_cursor_position((
                fields
                    .x
                    .saturating_add(offset)
                    .min(fields.right().saturating_sub(1)),
                fields.y.saturating_add(row as u16),
            ));
        }

        let lines: Vec<Line> = rows
            .into_iter()
            .map(|(prefix, value)| Line::from(format!("{prefix}{value}")))
            .collect();
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
            Paragraph::new(Line::styled(
                format!("  {} {message}", theme::warn()),
                theme::danger(),
            ))
            .render(error, frame.buffer_mut());
        }

        let cycle_hint = match self.focus {
            Field::Prefix | Field::Deps | Field::Submodules => "Cycle: ←→ | ",
            _ => "",
        };
        Paragraph::new(
            Line::from(format!(
                " Field: <tab> | {cycle_hint}Create: <enter> | Cancel: <esc>"
            ))
            .dim(),
        )
        .render(footer, frame.buffer_mut());
    }

    fn handle_event_key(&mut self, key_event: KeyEvent) -> KeyEventResponse {
        if key_event.kind != KeyEventKind::Press {
            return KeyEventResponse::Ignored;
        }
        if key_event.modifiers.contains(KeyModifiers::CONTROL) {
            return self.handle_control_key(key_event.code);
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
                self.move_or_cycle(1);
                KeyEventResponse::Consumed
            }
            KeyCode::Left => {
                self.move_or_cycle(-1);
                KeyEventResponse::Consumed
            }
            KeyCode::Home => {
                if let Some(input) = self.focused_text_mut() {
                    input.home();
                }
                KeyEventResponse::Consumed
            }
            KeyCode::End => {
                if let Some(input) = self.focused_text_mut() {
                    input.end();
                }
                KeyEventResponse::Consumed
            }
            KeyCode::Backspace => {
                self.pop_char();
                KeyEventResponse::Consumed
            }
            KeyCode::Delete => {
                self.delete_char();
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
            false,
        )
    }

    fn form_with_submodules() -> CreateForm {
        CreateForm::new(
            String::from("spectra"),
            vec![String::from("feature/")],
            String::from("develop"),
            vec![Strategy::Install, Strategy::None],
            true,
        )
    }

    fn form_without_deps() -> CreateForm {
        CreateForm::new(
            String::from("spectra"),
            vec![String::from("feature/")],
            String::from("develop"),
            vec![],
            false,
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
        assert!(text.contains("install"), "{text}");
        assert!(text.contains(theme::warn()), "{text}");
    }

    #[test]
    fn test_render_marks_the_focused_field_with_an_icon() {
        let text = dump(&mut form());
        assert!(text.contains(&format!("{} slug", theme::focus())), "{text}");
    }

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    #[test]
    fn test_left_moves_the_slug_cursor_instead_of_cycling_the_prefix() {
        let mut form = form();
        type_str(&mut form, "abc");
        form.handle_event_key(key(KeyCode::Left));
        form.handle_event_key(key(KeyCode::Char('X')));
        assert_eq!(form.branch().as_deref(), Some("feature/abXc"));
        assert_eq!(form.prefix(), "feature/");
    }

    #[test]
    fn test_a_typed_space_becomes_a_dash_in_the_slug() {
        let mut form = form();
        type_str(&mut form, "spe 11667");
        assert_eq!(form.branch().as_deref(), Some("feature/spe-11667"));
    }

    #[test]
    fn test_home_and_end_walk_the_slug_cursor_to_the_edges() {
        let mut form = form();
        type_str(&mut form, "bc");
        form.handle_event_key(key(KeyCode::Home));
        form.handle_event_key(key(KeyCode::Char('a')));
        form.handle_event_key(key(KeyCode::End));
        form.handle_event_key(key(KeyCode::Char('d')));
        assert_eq!(form.branch().as_deref(), Some("feature/abcd"));
    }

    #[test]
    fn test_delete_removes_the_character_under_the_slug_cursor() {
        let mut form = form();
        type_str(&mut form, "abc");
        form.handle_event_key(key(KeyCode::Home));
        form.handle_event_key(key(KeyCode::Delete));
        assert_eq!(form.branch().as_deref(), Some("feature/bc"));
    }

    #[test]
    fn test_ctrl_w_drops_the_last_slug_segment() {
        let mut form = form();
        type_str(&mut form, "spe-11667");
        form.handle_event_key(ctrl(KeyCode::Char('w')));
        assert_eq!(form.branch().as_deref(), Some("feature/spe-"));
    }

    #[test]
    fn test_ctrl_a_and_ctrl_e_jump_the_slug_cursor() {
        let mut form = form();
        type_str(&mut form, "bc");
        form.handle_event_key(ctrl(KeyCode::Char('a')));
        form.handle_event_key(key(KeyCode::Char('a')));
        form.handle_event_key(ctrl(KeyCode::Char('e')));
        form.handle_event_key(key(KeyCode::Char('d')));
        assert_eq!(form.branch().as_deref(), Some("feature/abcd"));
    }

    #[test]
    fn test_the_base_field_is_editable_too() {
        let mut form = form();
        form.handle_event_key(key(KeyCode::Tab));
        form.handle_event_key(key(KeyCode::Tab));
        assert_eq!(form.focus(), Field::Base);
        form.handle_event_key(key(KeyCode::Home));
        form.handle_event_key(key(KeyCode::Char('x')));
        assert_eq!(form.base(), "xdevelop");
    }

    #[test]
    fn test_the_terminal_cursor_follows_the_slug_cursor_left() {
        let mut form = form();
        type_str(&mut form, "spe-11667");
        form.handle_event_key(key(KeyCode::Left));
        form.handle_event_key(key(KeyCode::Left));
        let terminal = render_to_terminal(&mut form);
        // Two cells back from the end-of-value column asserted above.
        assert_eq!(
            terminal.backend().cursor_position(),
            Position::from((21, 1))
        );
    }

    #[test]
    fn test_the_focused_slug_field_holds_the_terminal_cursor() {
        let mut form = form();
        type_str(&mut form, "spe-11667");
        let terminal = render_to_terminal(&mut form);
        assert!(terminal.backend().cursor_visible());
        // "> slug       spe-11667" is 22 cells, starting one cell inside the border.
        assert_eq!(
            terminal.backend().cursor_position(),
            Position::from((23, 1))
        );
    }

    #[test]
    fn test_an_empty_slug_field_still_holds_the_cursor() {
        let terminal = render_to_terminal(&mut form());
        assert!(terminal.backend().cursor_visible());
        assert_eq!(
            terminal.backend().cursor_position(),
            Position::from((14, 1))
        );
    }

    #[test]
    fn test_the_cursor_follows_focus_to_the_base_field() {
        let mut form = form();
        form.handle_event_key(key(KeyCode::Down));
        form.handle_event_key(key(KeyCode::Down));
        let terminal = render_to_terminal(&mut form);
        assert!(terminal.backend().cursor_visible());
        // "> base       develop" is 20 cells, on the third field row.
        assert_eq!(
            terminal.backend().cursor_position(),
            Position::from((21, 3))
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
    fn test_render_footer_hides_the_cycle_hint_on_a_typed_field() {
        let text = dump(&mut form());
        assert!(text.contains("Field: <tab>"), "{text}");
        assert!(!text.contains("Cycle: ←→"), "{text}");
    }

    #[test]
    fn test_render_footer_shows_the_cycle_hint_on_a_cycled_field() {
        let mut form = form();
        form.handle_event_key(key(KeyCode::Tab));
        let text = dump(&mut form);
        assert!(text.contains("Cycle: ←→"), "{text}");
    }

    #[test]
    fn test_render_labels_the_deps_row_with_the_strategy() {
        let mut form = form();
        let text = dump(&mut form);
        assert!(text.contains("deps"), "{text}");
        assert!(text.contains("install"), "{text}");
        assert!(!text.contains("modules"), "{text}");
    }

    #[test]
    fn test_render_omits_the_deps_row_when_install_is_unavailable() {
        let mut form = CreateForm::new(
            String::from("spectra"),
            vec![String::from("feature/")],
            String::from("develop"),
            vec![Strategy::None],
            false,
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
            false,
        );
        assert_eq!(form.focus(), Field::Slug);
        form.handle_event_key(key(KeyCode::Tab));
        assert_eq!(form.focus(), Field::Prefix);
        form.handle_event_key(key(KeyCode::Tab));
        assert_eq!(form.focus(), Field::Base);
        form.handle_event_key(key(KeyCode::Tab));
        assert_eq!(form.focus(), Field::Slug);
    }

    #[test]
    fn test_submodules_default_to_being_initialised() {
        assert!(form_with_submodules().submodules());
    }

    #[test]
    fn test_a_repo_without_a_gitmodules_never_initialises_submodules() {
        assert!(!form().submodules());
    }

    #[test]
    fn test_submodules_cycle_off_with_the_arrow_keys() {
        let mut form = form_with_submodules();
        while form.focus() != Field::Submodules {
            form.handle_event_key(key(KeyCode::Tab));
        }

        form.handle_event_key(key(KeyCode::Right));

        assert!(!form.submodules());
    }

    #[test]
    fn test_render_shows_the_submodules_row_for_a_superproject() {
        let text = dump(&mut form_with_submodules());
        assert!(text.contains("submodules"), "{text}");
        assert!(text.contains("init"), "{text}");
    }

    #[test]
    fn test_render_omits_the_submodules_row_without_a_gitmodules() {
        let text = dump(&mut form());
        assert!(!text.contains("submodules"), "{text}");
    }

    #[test]
    fn test_focus_reaches_submodules_after_deps() {
        let mut form = form_with_submodules();
        for _ in 0..3 {
            form.handle_event_key(key(KeyCode::Tab));
        }
        assert_eq!(form.focus(), Field::Deps);
        form.handle_event_key(key(KeyCode::Tab));
        assert_eq!(form.focus(), Field::Submodules);
        form.handle_event_key(key(KeyCode::Tab));
        assert_eq!(form.focus(), Field::Slug);
    }
}
