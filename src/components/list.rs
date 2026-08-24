use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, StatefulWidget, Widget};

use crate::components::{Component, KeyEventResponse, fit_tail};

pub struct Row {
    pub label: String,
    pub branch: String,
    /// `None` until the background scan reports on this worktree.
    pub dirty: Option<bool>,
    pub path: PathBuf,
}

pub struct ListComponent {
    repo_name: String,
    rows: Vec<Row>,
    /// Indices into `rows` that survive the active filter, in display order.
    visible: Vec<usize>,
    /// The query `enter` committed.
    query: String,
    /// The live query while the search bar is open; `esc` drops it back to `query`.
    editing: Option<String>,
    state: ListState,
    shell_init: bool,
}

impl ListComponent {
    pub fn new(repo_name: String, rows: Vec<Row>, shell_init: bool) -> Self {
        let mut component = Self {
            repo_name,
            visible: (0..rows.len()).collect(),
            rows,
            query: String::new(),
            editing: None,
            state: ListState::default(),
            shell_init,
        };
        component.refilter();
        component
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.rows.iter().map(|row| row.path.clone()).collect()
    }

    pub fn set_dirty(&mut self, path: &Path, dirty: bool) {
        if let Some(row) = self.rows.iter_mut().find(|row| row.path == path) {
            row.dirty = Some(dirty);
        }
    }

    pub fn selected_path(&self) -> Option<PathBuf> {
        let row = *self.visible.get(self.state.selected()?)?;
        self.rows.get(row).map(|row| row.path.clone())
    }

    fn filter(&self) -> &str {
        self.editing.as_deref().unwrap_or(&self.query)
    }

    fn refilter(&mut self) {
        let needle = self.filter().to_lowercase();
        self.visible = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                needle.is_empty()
                    || row.label.to_lowercase().contains(&needle)
                    || row.branch.to_lowercase().contains(&needle)
            })
            .map(|(index, _)| index)
            .collect();
        self.state.select((!self.visible.is_empty()).then_some(0));
    }

    fn search_bar(&self) -> Option<String> {
        self.editing.as_deref().map(|buffer| format!(" /{buffer}"))
    }

    fn footer_line(&self) -> Line<'_> {
        if let Some(bar) = self.search_bar() {
            return Line::from(vec![
                Span::from(bar),
                Span::from("    enter select    esc cancel").dim(),
            ]);
        }
        let cd_hint = if self.shell_init {
            "enter cd"
        } else {
            "enter … (needs shell init)"
        };
        let clear_hint = if self.query.is_empty() {
            "/ search"
        } else {
            "esc clear"
        };
        Line::from(format!(
            " ↑↓/jk move    n new    {cd_hint}    {clear_hint}    q quit"
        ))
        .dim()
    }

    /// Swallows every key so list and app shortcuts cannot fire mid-query.
    fn handle_search_key(&mut self, key_event: KeyEvent) -> KeyEventResponse {
        let Some(buffer) = self.editing.as_mut() else {
            return KeyEventResponse::Ignored;
        };
        match key_event.code {
            KeyCode::Char(character) => {
                buffer.push(character);
                self.refilter();
            }
            KeyCode::Backspace => {
                buffer.pop();
                self.refilter();
            }
            KeyCode::Enter => {
                self.query = self.editing.take().unwrap_or_default();
                self.refilter();
            }
            KeyCode::Esc => {
                self.editing = None;
                self.refilter();
            }
            _ => {}
        }
        KeyEventResponse::Consumed
    }

    fn move_selection(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let last = self.visible.len().saturating_sub(1);
        let current = self.state.selected().unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, last as isize) as usize;
        self.state.select(Some(next));
    }
}

impl Component for ListComponent {
    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let [body, footer] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

        let inner = body.width.saturating_sub(2) as usize;
        let label_width = self
            .rows
            .iter()
            .map(|row| row.label.chars().count())
            .max()
            .unwrap_or(0)
            .clamp(12, 40);
        let branch_width = inner.saturating_sub(2 + label_width + 1 + 2).max(8);

        let items: Vec<ListItem> = self
            .visible
            .iter()
            .filter_map(|index| self.rows.get(*index))
            .map(|row| {
                let dirty = if row.dirty == Some(true) { "●" } else { " " };
                let nested = row.label.contains('/');
                let label = Span::from(format!(
                    "{:<label_width$} ",
                    fit_tail(&row.label, label_width)
                ));
                let label = if nested { label.dim() } else { label };
                let spans = vec![
                    label,
                    Span::from(format!(
                        "{:<branch_width$}",
                        fit_tail(&row.branch, branch_width)
                    )),
                    Span::from(format!("{dirty} ")),
                ];
                ListItem::new(Line::from(spans))
            })
            .collect();

        let title = if self.editing.is_none() && !self.query.is_empty() {
            format!(" {}  /{} ", self.repo_name, self.query)
        } else {
            format!(" {} ", self.repo_name)
        };
        let list = List::new(items)
            .block(Block::bordered().title(title))
            .highlight_symbol("> ");
        StatefulWidget::render(list, body, frame.buffer_mut(), &mut self.state);

        Paragraph::new(self.footer_line()).render(footer, frame.buffer_mut());

        // Only the open search bar asks for a cursor; unset elsewhere is what hides it.
        if let Some(bar) = self.search_bar() {
            let x = footer.x.saturating_add(bar.chars().count() as u16);
            frame.set_cursor_position((x.min(footer.right().saturating_sub(1)), footer.y));
        }
    }

    fn handle_event_key(&mut self, key_event: KeyEvent) -> KeyEventResponse {
        if key_event.kind != KeyEventKind::Press {
            return KeyEventResponse::Ignored;
        }
        if self.editing.is_some() {
            return self.handle_search_key(key_event);
        }
        match key_event.code {
            KeyCode::Char('/') => {
                self.editing = Some(self.query.clone());
                KeyEventResponse::Consumed
            }
            // A committed filter absorbs esc; an unfiltered list lets it through to quit.
            KeyCode::Esc if !self.query.is_empty() => {
                self.query.clear();
                self.refilter();
                KeyEventResponse::Consumed
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(1);
                KeyEventResponse::Consumed
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(-1);
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

    fn rows() -> Vec<Row> {
        vec![
            Row {
                label: String::from("spectra"),
                branch: String::from("develop"),
                dirty: Some(false),
                path: PathBuf::from("/w/spectra"),
            },
            Row {
                label: String::from("spectra-ter"),
                branch: String::from("ter"),
                dirty: Some(true),
                path: PathBuf::from("/w/spectra-ter"),
            },
        ]
    }

    fn unscanned_component() -> ListComponent {
        let rows = rows()
            .into_iter()
            .map(|row| Row { dirty: None, ..row })
            .collect();
        ListComponent::new(String::from("spectra"), rows, true)
    }

    fn component(shell_init: bool) -> ListComponent {
        ListComponent::new(String::from("spectra"), rows(), shell_init)
    }

    fn search(component: &mut ListComponent, query: &str) {
        component.handle_event_key(key(KeyCode::Char('/')));
        for character in query.chars() {
            component.handle_event_key(key(KeyCode::Char(character)));
        }
    }

    fn render_to_terminal(component: &mut ListComponent) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(70, 8)).unwrap();
        terminal
            .draw(|frame| component.render(frame, frame.area()))
            .unwrap();
        terminal
    }

    fn dump(component: &mut ListComponent) -> String {
        buffer_to_string(render_to_terminal(component).backend().buffer())
    }

    #[test]
    fn test_new_selects_the_first_row() {
        let component = component(true);
        assert_eq!(component.selected_path(), Some(PathBuf::from("/w/spectra")));
    }

    #[test]
    fn test_j_moves_selection_down() {
        let mut component = component(true);
        assert!(matches!(
            component.handle_event_key(key(KeyCode::Char('j'))),
            KeyEventResponse::Consumed
        ));
        assert_eq!(
            component.selected_path(),
            Some(PathBuf::from("/w/spectra-ter"))
        );
    }

    #[test]
    fn test_down_arrow_moves_selection_down() {
        let mut component = component(true);
        assert!(matches!(
            component.handle_event_key(key(KeyCode::Down)),
            KeyEventResponse::Consumed
        ));
        assert_eq!(
            component.selected_path(),
            Some(PathBuf::from("/w/spectra-ter"))
        );
    }

    #[test]
    fn test_up_arrow_moves_selection_up() {
        let mut component = component(true);
        component.handle_event_key(key(KeyCode::Down));
        assert!(matches!(
            component.handle_event_key(key(KeyCode::Up)),
            KeyEventResponse::Consumed
        ));
        assert_eq!(component.selected_path(), Some(PathBuf::from("/w/spectra")));
    }

    #[test]
    fn test_render_footer_shows_both_movement_forms() {
        let text = dump(&mut component(true));
        assert!(text.contains("↑↓/jk move"), "{text}");
    }

    #[test]
    fn test_k_moves_selection_up() {
        let mut component = component(true);
        component.handle_event_key(key(KeyCode::Char('j')));
        component.handle_event_key(key(KeyCode::Char('k')));
        assert_eq!(component.selected_path(), Some(PathBuf::from("/w/spectra")));
    }

    #[test]
    fn test_selection_does_not_move_past_the_last_row() {
        let mut component = component(true);
        component.handle_event_key(key(KeyCode::Char('j')));
        component.handle_event_key(key(KeyCode::Char('j')));
        assert_eq!(
            component.selected_path(),
            Some(PathBuf::from("/w/spectra-ter"))
        );
    }

    #[test]
    fn test_unhandled_key_is_ignored() {
        let mut component = component(true);
        assert!(matches!(
            component.handle_event_key(key(KeyCode::Char('z'))),
            KeyEventResponse::Ignored
        ));
    }

    #[test]
    fn test_key_release_is_ignored() {
        let mut component = component(true);
        let mut event = key(KeyCode::Char('j'));
        event.kind = KeyEventKind::Release;
        assert!(matches!(
            component.handle_event_key(event),
            KeyEventResponse::Ignored
        ));
        assert_eq!(component.selected_path(), Some(PathBuf::from("/w/spectra")));
    }

    #[test]
    fn test_empty_rows_have_no_selection() {
        let component = ListComponent::new(String::from("spectra"), vec![], true);
        assert_eq!(component.selected_path(), None);
    }

    #[test]
    fn test_render_shows_repo_name_branches_and_dirty_marker() {
        let text = dump(&mut component(true));
        assert!(text.contains("spectra"), "{text}");
        assert!(text.contains("develop"), "{text}");
        assert!(text.contains('●'), "{text}");
        assert!(text.contains("enter cd"), "{text}");
    }

    #[test]
    fn test_rows_start_with_an_unknown_dirty_state() {
        let component = ListComponent::new(
            String::from("spectra"),
            vec![Row {
                label: String::from("spectra"),
                branch: String::from("develop"),
                dirty: None,
                path: PathBuf::from("/w/spectra"),
            }],
            true,
        );
        assert_eq!(component.rows[0].dirty, None);
    }

    #[test]
    fn test_unknown_dirty_state_renders_no_marker() {
        let mut component = ListComponent::new(
            String::from("spectra"),
            vec![Row {
                label: String::from("spectra"),
                branch: String::from("develop"),
                dirty: None,
                path: PathBuf::from("/w/spectra"),
            }],
            true,
        );
        let text = dump(&mut component);
        assert!(!text.contains('\u{25cf}'), "{text}");
    }

    #[test]
    fn test_set_dirty_marks_the_row_with_the_matching_path() {
        let mut component = unscanned_component();
        component.set_dirty(Path::new("/w/spectra"), true);
        assert_eq!(component.rows[0].dirty, Some(true));
        assert_eq!(component.rows[1].dirty, None);
    }

    #[test]
    fn test_set_dirty_ignores_a_path_that_is_not_listed() {
        let mut component = unscanned_component();
        component.set_dirty(Path::new("/w/nowhere"), true);
        assert!(component.rows.iter().all(|row| row.dirty.is_none()));
    }

    #[test]
    fn test_render_without_shell_init_shows_setup_hint() {
        let text = dump(&mut component(false));
        assert!(text.contains("needs shell init"), "{text}");
    }

    #[test]
    fn test_fit_tail_leaves_a_string_that_already_fits() {
        assert_eq!(fit_tail("spectra", 12), "spectra");
    }

    #[test]
    fn test_fit_tail_keeps_the_end_of_an_overlong_string() {
        assert_eq!(
            fit_tail("refactor/spe-11667-input-toggle", 10),
            "…ut-toggle"
        );
    }

    #[test]
    fn test_fit_tail_counts_characters_not_bytes() {
        assert_eq!(fit_tail("é".repeat(6).as_str(), 6).chars().count(), 6);
    }

    fn screen_lines(text: &str, width: usize) -> Vec<String> {
        text.chars()
            .collect::<Vec<_>>()
            .chunks(width)
            .map(|chunk| chunk.iter().collect())
            .collect()
    }

    #[test]
    fn test_render_keeps_the_branch_column_aligned_when_labels_are_long() {
        let long_rows = vec![
            Row {
                label: String::from(".claude/worktrees/input-button-counter"),
                branch: String::from("BRANCH-A"),
                dirty: None,
                path: PathBuf::from("/w/a"),
            },
            Row {
                label: String::from(".claude/worktrees/input-collapsible-container"),
                branch: String::from("BRANCH-B"),
                dirty: None,
                path: PathBuf::from("/w/b"),
            },
        ];
        let mut component = ListComponent::new(String::from("r"), long_rows, true);
        let text = dump(&mut component);
        let lines = screen_lines(&text, 70);

        let column_of = |line: &str, needle: &str| -> Option<usize> {
            let byte = line.find(needle)?;
            Some(line[..byte].chars().count())
        };
        let a = lines.iter().find(|l| l.contains("BRANCH-A")).unwrap();
        let b = lines.iter().find(|l| l.contains("BRANCH-B")).unwrap();
        assert_eq!(
            column_of(a, "BRANCH-A"),
            column_of(b, "BRANCH-B"),
            "branch column misaligned:\n{a}\n{b}"
        );
        assert!(text.contains('…'), "{text}");
    }

    #[test]
    fn test_typing_a_query_selects_the_first_matching_row() {
        let mut component = component(true);
        search(&mut component, "ter");
        assert_eq!(
            component.selected_path(),
            Some(PathBuf::from("/w/spectra-ter"))
        );
    }

    #[test]
    fn test_typing_a_query_hides_the_rows_that_do_not_match() {
        let mut component = component(true);
        search(&mut component, "ter");
        let text = dump(&mut component);
        assert!(text.contains("spectra-ter"), "{text}");
        assert!(!text.contains("develop"), "{text}");
    }

    #[test]
    fn test_a_query_matches_the_branch_column() {
        let mut component = component(true);
        search(&mut component, "develop");
        assert_eq!(component.selected_path(), Some(PathBuf::from("/w/spectra")));
        let text = dump(&mut component);
        assert!(!text.contains("spectra-ter"), "{text}");
    }

    #[test]
    fn test_a_query_ignores_case() {
        let mut component = component(true);
        search(&mut component, "TER");
        assert_eq!(
            component.selected_path(),
            Some(PathBuf::from("/w/spectra-ter"))
        );
    }

    #[test]
    fn test_a_query_that_matches_nothing_leaves_no_selection() {
        let mut component = component(true);
        search(&mut component, "zzz");
        assert_eq!(component.selected_path(), None);
    }

    #[test]
    fn test_typed_characters_do_not_move_the_selection() {
        let mut component = component(true);
        search(&mut component, "j");
        assert_eq!(component.selected_path(), None);
    }

    #[test]
    fn test_esc_while_searching_restores_every_row() {
        let mut component = component(true);
        search(&mut component, "ter");
        assert!(matches!(
            component.handle_event_key(key(KeyCode::Esc)),
            KeyEventResponse::Consumed
        ));
        assert_eq!(component.selected_path(), Some(PathBuf::from("/w/spectra")));
        let text = dump(&mut component);
        assert!(text.contains("develop"), "{text}");
    }

    #[test]
    fn test_enter_keeps_the_filter_applied() {
        let mut component = component(true);
        search(&mut component, "ter");
        assert!(matches!(
            component.handle_event_key(key(KeyCode::Enter)),
            KeyEventResponse::Consumed
        ));
        assert_eq!(
            component.selected_path(),
            Some(PathBuf::from("/w/spectra-ter"))
        );
        let text = dump(&mut component);
        assert!(!text.contains("develop"), "{text}");
    }

    #[test]
    fn test_enter_closes_the_bar_so_movement_keys_work_again() {
        let mut component = component(true);
        search(&mut component, "spectra");
        component.handle_event_key(key(KeyCode::Enter));
        component.handle_event_key(key(KeyCode::Char('j')));
        assert_eq!(
            component.selected_path(),
            Some(PathBuf::from("/w/spectra-ter"))
        );
    }

    #[test]
    fn test_esc_after_a_committed_query_clears_the_filter() {
        let mut component = component(true);
        search(&mut component, "ter");
        component.handle_event_key(key(KeyCode::Enter));
        assert!(matches!(
            component.handle_event_key(key(KeyCode::Esc)),
            KeyEventResponse::Consumed
        ));
        assert_eq!(component.selected_path(), Some(PathBuf::from("/w/spectra")));
        let text = dump(&mut component);
        assert!(text.contains("develop"), "{text}");
    }

    #[test]
    fn test_esc_on_an_unfiltered_list_is_ignored_so_the_app_can_quit() {
        let mut component = component(true);
        assert!(matches!(
            component.handle_event_key(key(KeyCode::Esc)),
            KeyEventResponse::Ignored
        ));
    }

    #[test]
    fn test_backspace_widens_the_filter_again() {
        let mut component = component(true);
        search(&mut component, "terz");
        assert_eq!(component.selected_path(), None);
        component.handle_event_key(key(KeyCode::Backspace));
        assert_eq!(
            component.selected_path(),
            Some(PathBuf::from("/w/spectra-ter"))
        );
    }

    #[test]
    fn test_slash_reopens_the_bar_with_the_committed_query() {
        let mut component = component(true);
        search(&mut component, "ter");
        component.handle_event_key(key(KeyCode::Enter));
        search(&mut component, "s");
        assert_eq!(component.selected_path(), None, "query should be 'ters'");
    }

    #[test]
    fn test_esc_after_reopening_restores_the_committed_query() {
        let mut component = component(true);
        search(&mut component, "ter");
        component.handle_event_key(key(KeyCode::Enter));
        search(&mut component, "s");
        component.handle_event_key(key(KeyCode::Esc));
        assert_eq!(
            component.selected_path(),
            Some(PathBuf::from("/w/spectra-ter"))
        );
        let text = dump(&mut component);
        assert!(!text.contains("develop"), "{text}");
    }

    #[test]
    fn test_render_shows_the_query_and_a_cancel_hint_while_searching() {
        let mut component = component(true);
        search(&mut component, "ter");
        let text = dump(&mut component);
        assert!(text.contains("/ter"), "{text}");
        assert!(text.contains("esc cancel"), "{text}");
        assert!(!text.contains("q quit"), "{text}");
    }

    #[test]
    fn test_the_search_bar_parks_the_terminal_cursor_after_the_query() {
        let mut component = component(true);
        search(&mut component, "ter");
        let terminal = render_to_terminal(&mut component);
        assert!(terminal.backend().cursor_visible());
        // " /ter" is five cells wide, and the footer is the last row of the 70x8 area.
        assert_eq!(terminal.backend().cursor_position(), Position::from((5, 7)));
    }

    #[test]
    fn test_closing_the_search_bar_hides_the_terminal_cursor() {
        let mut component = component(true);
        search(&mut component, "ter");
        component.handle_event_key(key(KeyCode::Enter));
        let terminal = render_to_terminal(&mut component);
        assert!(!terminal.backend().cursor_visible());
    }

    #[test]
    fn test_an_unsearched_list_hides_the_terminal_cursor() {
        let terminal = render_to_terminal(&mut component(true));
        assert!(!terminal.backend().cursor_visible());
    }

    #[test]
    fn test_the_cursor_stays_inside_the_footer_when_the_query_overflows() {
        let mut component = component(true);
        search(&mut component, &"x".repeat(200));
        let terminal = render_to_terminal(&mut component);
        assert!(terminal.backend().cursor_position().x < 70);
    }

    #[test]
    fn test_render_offers_esc_clear_once_a_filter_is_committed() {
        let mut component = component(true);
        search(&mut component, "ter");
        component.handle_event_key(key(KeyCode::Enter));
        let text = dump(&mut component);
        assert!(text.contains("esc clear"), "{text}");
    }

    #[test]
    fn test_render_shows_the_committed_query_in_the_title() {
        let mut component = component(true);
        search(&mut component, "ter");
        component.handle_event_key(key(KeyCode::Enter));
        let text = dump(&mut component);
        assert!(text.contains("spectra  /ter"), "{text}");
    }

    #[test]
    fn test_render_advertises_the_search_key_on_an_unfiltered_list() {
        let text = dump(&mut component(true));
        assert!(text.contains("/ search"), "{text}");
    }

    #[test]
    fn test_render_shows_no_modules_column() {
        let text = dump(&mut component(true));
        assert!(!text.contains("link"), "{text}");
        assert!(!text.contains("own"), "{text}");
        assert!(text.contains("develop"), "{text}");
        assert!(text.contains('●'), "{text}");
    }
}
