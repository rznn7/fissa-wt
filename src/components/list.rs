use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, StatefulWidget, Widget};

use crate::components::text_input::TextInput;
use crate::components::theme;
use crate::components::{Component, KeyEventResponse, fit_tail};

pub struct Row {
    pub label: String,
    /// `None` for a detached worktree.
    pub branch: Option<String>,
    /// `None` until the background scan reports on this worktree.
    pub dirty: Option<bool>,
    pub path: PathBuf,
}

impl Row {
    pub fn branch_or_detached(&self) -> &str {
        self.branch.as_deref().unwrap_or("(detached)")
    }
}

pub struct ListComponent {
    repo_name: String,
    rows: Vec<Row>,
    /// Indices into `rows` that survive the active filter, in display order.
    visible: Vec<usize>,
    /// The query `enter` committed.
    query: String,
    /// The live query while the search bar is open; `esc` drops it back to `query`.
    editing: Option<TextInput>,
    state: ListState,
    /// Where a shift-extended range started, as an index into `visible`.
    anchor: Option<usize>,
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
            anchor: None,
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

    /// The rows a delete would act on: the whole open range, or the cursor row.
    pub fn marked_rows(&self) -> Vec<&Row> {
        self.marked_slots()
            .filter_map(|slot| self.visible.get(slot))
            .filter_map(|index| self.rows.get(*index))
            .collect()
    }

    fn marked_slots(&self) -> std::ops::Range<usize> {
        let Some(cursor) = self.state.selected() else {
            return 0..0;
        };
        let anchor = self.anchor.unwrap_or(cursor);
        anchor.min(cursor)..anchor.max(cursor) + 1
    }

    fn filter(&self) -> &str {
        self.editing
            .as_ref()
            .map(TextInput::value)
            .unwrap_or(&self.query)
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
                    || row.branch_or_detached().to_lowercase().contains(&needle)
            })
            .map(|(index, _)| index)
            .collect();
        self.state.select((!self.visible.is_empty()).then_some(0));
        self.anchor = None;
    }

    fn search_bar(&self) -> Option<String> {
        self.editing
            .as_ref()
            .map(|input| format!(" {} {}", theme::SEARCH, input.value()))
    }

    /// Where the caret sits in the search bar, as a column offset from the footer.
    fn search_cursor_offset(&self) -> Option<usize> {
        let input = self.editing.as_ref()?;
        let prefix = format!(" {} ", theme::SEARCH).chars().count();
        Some(prefix + input.cursor())
    }

    fn footer_line(&self) -> Line<'_> {
        if let Some(bar) = self.search_bar() {
            return Line::from(vec![
                Span::from(bar),
                Span::from("    Select: <enter> | Cancel: <esc>").dim(),
            ]);
        }
        let cd_hint = if self.shell_init {
            "Cd: <enter>"
        } else {
            "Cd: … (needs shell init)"
        };
        let filter_hint = if self.query.is_empty() {
            "Search: /"
        } else {
            "Clear: <esc>"
        };
        Line::from(format!(
            " Move: ↑↓/jk | New: n | {cd_hint} | Delete: d | {filter_hint} | Quit: q"
        ))
        .dim()
    }

    /// Swallows every key so list and app shortcuts cannot fire mid-query.
    fn handle_search_key(&mut self, key_event: KeyEvent) -> KeyEventResponse {
        let Some(input) = self.editing.as_mut() else {
            return KeyEventResponse::Ignored;
        };
        let control = key_event.modifiers.contains(KeyModifiers::CONTROL);
        match key_event.code {
            KeyCode::Char('a') if control => input.home(),
            KeyCode::Char('e') if control => input.end(),
            KeyCode::Char('w') if control => {
                input.delete_word_back();
                self.refilter();
            }
            KeyCode::Char(_) if control => {}
            KeyCode::Char(character) => {
                input.insert(character);
                self.refilter();
            }
            KeyCode::Backspace => {
                input.backspace();
                self.refilter();
            }
            KeyCode::Delete => {
                input.delete();
                self.refilter();
            }
            KeyCode::Left => input.left(),
            KeyCode::Right => input.right(),
            KeyCode::Home => input.home(),
            KeyCode::End => input.end(),
            KeyCode::Enter => {
                self.query = self
                    .editing
                    .take()
                    .map(|input| input.value().to_string())
                    .unwrap_or_default();
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

    fn move_selection(&mut self, delta: isize, extend: bool) {
        if self.visible.is_empty() {
            return;
        }
        if extend {
            self.anchor = self.anchor.or(self.state.selected());
        } else {
            self.anchor = None;
        }
        let last = self.visible.len().saturating_sub(1);
        let current = self.state.selected().unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, last as isize) as usize;
        self.state.select(Some(next));
    }
}

fn extends_range(key_event: KeyEvent) -> bool {
    key_event.modifiers.contains(KeyModifiers::SHIFT)
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
        let gutters = 2 + 2 + 1 + 2 + 2;
        let branch_width = inner.saturating_sub(gutters + label_width).max(8);

        // An open search bar owns the focus, so the list drops its selection feedback.
        let focused = self.editing.is_none();
        let marked = if focused { self.marked_slots() } else { 0..0 };
        let items: Vec<ListItem> = self
            .visible
            .iter()
            .enumerate()
            .filter_map(|(slot, index)| Some((slot, self.rows.get(*index)?)))
            .map(|(slot, row)| {
                let nested = row.label.contains('/');
                let icon = Span::from(format!("{} ", theme::WORKTREE));
                let icon = if nested { icon.dim() } else { icon };
                let label = Span::from(format!(
                    "{:<label_width$} ",
                    fit_tail(&row.label, label_width)
                ));
                let label = if nested { label.dim() } else { label };
                let spans = vec![
                    if row.dirty == Some(true) {
                        Span::styled(format!("{} ", theme::DIRTY), theme::dirty())
                    } else {
                        Span::from("  ")
                    },
                    icon,
                    label,
                    Span::from(format!("{} ", theme::BRANCH)).dim(),
                    Span::from(format!(
                        "{:<branch_width$}",
                        fit_tail(row.branch_or_detached(), branch_width)
                    )),
                ];
                let item = ListItem::new(Line::from(spans));
                if marked.contains(&slot) {
                    item.style(theme::selection())
                } else {
                    item
                }
            })
            .collect();

        let mut title = vec![
            Span::from(" "),
            Span::styled(self.repo_name.as_str(), theme::title()),
        ];
        if self.editing.is_none() && !self.query.is_empty() {
            title.push(Span::styled(
                format!("  {} {}", theme::SEARCH, self.query),
                theme::accent(),
            ));
        }
        title.push(Span::from(" "));

        let list = List::new(items)
            .block(Block::bordered().title(Line::from(title)))
            // The symbol stays either way so the columns do not shift when the bar opens.
            .highlight_symbol("  ")
            .highlight_style(if focused {
                theme::selection()
            } else {
                Style::new()
            });
        StatefulWidget::render(list, body, frame.buffer_mut(), &mut self.state);

        Paragraph::new(self.footer_line()).render(footer, frame.buffer_mut());

        // Only the open search bar asks for a cursor; unset elsewhere is what hides it.
        if let Some(offset) = self.search_cursor_offset() {
            let x = footer.x.saturating_add(offset as u16);
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
                self.editing = Some(TextInput::new(self.query.clone()));
                KeyEventResponse::Consumed
            }
            KeyCode::Esc if self.anchor.is_some() => {
                self.anchor = None;
                KeyEventResponse::Consumed
            }
            // A committed filter absorbs esc; an unfiltered list lets it through to quit.
            KeyCode::Esc if !self.query.is_empty() => {
                self.query.clear();
                self.refilter();
                KeyEventResponse::Consumed
            }
            KeyCode::Char('J') | KeyCode::Down if extends_range(key_event) => {
                self.move_selection(1, true);
                KeyEventResponse::Consumed
            }
            KeyCode::Char('K') | KeyCode::Up if extends_range(key_event) => {
                self.move_selection(-1, true);
                KeyEventResponse::Consumed
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(1, false);
                KeyEventResponse::Consumed
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(-1, false);
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
                branch: Some(String::from("develop")),
                dirty: Some(false),
                path: PathBuf::from("/w/spectra"),
            },
            Row {
                label: String::from("spectra-ter"),
                branch: Some(String::from("ter")),
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

    const TERMINAL_WIDTH: u16 = 80;

    fn render_to_terminal(component: &mut ListComponent) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(TERMINAL_WIDTH, 8)).unwrap();
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
        assert!(text.contains("Move: ↑↓/jk"), "{text}");
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
        assert!(text.contains(theme::DIRTY), "{text}");
        assert!(text.contains("Cd: <enter>"), "{text}");
    }

    #[test]
    fn test_rows_start_with_an_unknown_dirty_state() {
        let component = ListComponent::new(
            String::from("spectra"),
            vec![Row {
                label: String::from("spectra"),
                branch: Some(String::from("develop")),
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
                branch: Some(String::from("develop")),
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
    fn test_render_calls_a_branchless_worktree_detached() {
        let mut component = ListComponent::new(
            String::from("spectra"),
            vec![Row {
                label: String::from("spectra"),
                branch: None,
                dirty: None,
                path: PathBuf::from("/w/spectra"),
            }],
            true,
        );
        assert!(dump(&mut component).contains("(detached)"));
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
                branch: Some(String::from("BRANCH-A")),
                dirty: None,
                path: PathBuf::from("/w/a"),
            },
            Row {
                label: String::from(".claude/worktrees/input-collapsible-container"),
                branch: Some(String::from("BRANCH-B")),
                dirty: None,
                path: PathBuf::from("/w/b"),
            },
        ];
        let mut component = ListComponent::new(String::from("r"), long_rows, true);
        let text = dump(&mut component);
        let lines = screen_lines(&text, TERMINAL_WIDTH as usize);

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
        assert!(text.contains(&format!("{} ter", theme::SEARCH)), "{text}");
        assert!(text.contains("Cancel: <esc>"), "{text}");
        assert!(!text.contains("Quit: q"), "{text}");
    }

    #[test]
    fn test_the_search_bar_parks_the_terminal_cursor_after_the_query() {
        let mut component = component(true);
        search(&mut component, "ter");
        let terminal = render_to_terminal(&mut component);
        assert!(terminal.backend().cursor_visible());
        // The search bar is six cells wide, and the footer is the last row of the 70x8 area.
        assert_eq!(terminal.backend().cursor_position(), Position::from((6, 7)));
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
        assert!(terminal.backend().cursor_position().x < TERMINAL_WIDTH);
    }

    #[test]
    fn test_render_offers_esc_clear_once_a_filter_is_committed() {
        let mut component = component(true);
        search(&mut component, "ter");
        component.handle_event_key(key(KeyCode::Enter));
        let text = dump(&mut component);
        assert!(text.contains("Clear: <esc>"), "{text}");
    }

    #[test]
    fn test_render_shows_the_committed_query_in_the_title() {
        let mut component = component(true);
        search(&mut component, "ter");
        component.handle_event_key(key(KeyCode::Enter));
        let text = dump(&mut component);
        assert!(
            text.contains(&format!("spectra  {} ter", theme::SEARCH)),
            "{text}"
        );
    }

    #[test]
    fn test_render_advertises_the_search_key_on_an_unfiltered_list() {
        let text = dump(&mut component(true));
        assert!(text.contains("Search: /"), "{text}");
    }

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    /// The live filter, read back through the rows it leaves visible.
    fn visible_labels(component: &ListComponent) -> Vec<&str> {
        component
            .visible
            .iter()
            .filter_map(|index| component.rows.get(*index))
            .map(|row| row.label.as_str())
            .collect()
    }

    #[test]
    fn test_left_then_typing_inserts_mid_query() {
        let mut component = component(true);
        search(&mut component, "tr");
        component.handle_event_key(key(KeyCode::Left));
        component.handle_event_key(key(KeyCode::Char('e')));
        assert_eq!(component.filter(), "ter");
        assert_eq!(visible_labels(&component), vec!["spectra-ter"]);
    }

    #[test]
    fn test_delete_removes_the_character_under_the_query_cursor() {
        let mut component = component(true);
        search(&mut component, "xter");
        component.handle_event_key(key(KeyCode::Home));
        component.handle_event_key(key(KeyCode::Delete));
        assert_eq!(component.filter(), "ter");
        assert_eq!(visible_labels(&component), vec!["spectra-ter"]);
    }

    #[test]
    fn test_home_and_end_walk_the_query_cursor_to_the_edges() {
        let mut component = component(true);
        search(&mut component, "ec");
        component.handle_event_key(key(KeyCode::Home));
        component.handle_event_key(key(KeyCode::Char('s')));
        component.handle_event_key(key(KeyCode::End));
        component.handle_event_key(key(KeyCode::Char('t')));
        assert_eq!(component.filter(), "sect");
    }

    #[test]
    fn test_ctrl_w_drops_the_last_query_segment() {
        let mut component = component(true);
        search(&mut component, "spectra-ter");
        component.handle_event_key(ctrl(KeyCode::Char('w')));
        assert_eq!(component.filter(), "spectra-");
        assert_eq!(visible_labels(&component), vec!["spectra-ter"]);
    }

    #[test]
    fn test_ctrl_a_and_ctrl_e_jump_the_query_cursor() {
        let mut component = component(true);
        search(&mut component, "ec");
        component.handle_event_key(ctrl(KeyCode::Char('a')));
        component.handle_event_key(key(KeyCode::Char('s')));
        component.handle_event_key(ctrl(KeyCode::Char('e')));
        component.handle_event_key(key(KeyCode::Char('t')));
        assert_eq!(component.filter(), "sect");
    }

    #[test]
    fn test_a_typed_space_becomes_a_dash_in_the_query() {
        let mut component = component(true);
        search(&mut component, "spectra ter");
        assert_eq!(component.filter(), "spectra-ter");
        assert_eq!(visible_labels(&component), vec!["spectra-ter"]);
    }

    #[test]
    fn test_an_unbound_control_key_neither_types_nor_escapes_the_bar() {
        let mut component = component(true);
        search(&mut component, "ter");
        component.handle_event_key(ctrl(KeyCode::Char('c')));
        assert_eq!(component.filter(), "ter");
        assert!(component.editing.is_some());
    }

    #[test]
    fn test_the_terminal_cursor_follows_the_query_cursor_left() {
        let mut component = component(true);
        search(&mut component, "ter");
        component.handle_event_key(key(KeyCode::Left));
        let terminal = render_to_terminal(&mut component);
        // One cell back from the end-of-query column asserted above.
        assert_eq!(terminal.backend().cursor_position(), Position::from((5, 7)));
    }

    #[test]
    fn test_enter_commits_the_edited_query_not_the_original() {
        let mut component = component(true);
        search(&mut component, "tr");
        component.handle_event_key(key(KeyCode::Left));
        component.handle_event_key(key(KeyCode::Char('e')));
        component.handle_event_key(key(KeyCode::Enter));
        assert_eq!(component.query, "ter");
        assert!(component.editing.is_none());
    }

    #[test]
    fn test_render_marks_each_row_with_a_worktree_and_branch_icon() {
        let text = dump(&mut component(true));
        assert!(text.contains(theme::WORKTREE), "{text}");
        assert!(text.contains(theme::BRANCH), "{text}");
    }

    #[test]
    fn test_render_shows_no_modules_column() {
        let text = dump(&mut component(true));
        assert!(!text.contains("link"), "{text}");
        assert!(!text.contains("own"), "{text}");
        assert!(text.contains("develop"), "{text}");
        assert!(text.contains(theme::DIRTY), "{text}");
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;
    use crate::components::{key, shift_key};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;

    fn component() -> ListComponent {
        let rows = ["one", "two", "three"]
            .iter()
            .map(|name| Row {
                label: String::from(*name),
                branch: Some(format!("branch-{name}")),
                dirty: None,
                path: PathBuf::from(format!("/w/{name}")),
            })
            .collect();
        ListComponent::new(String::from("repo"), rows, true)
    }

    fn marked(component: &ListComponent) -> Vec<PathBuf> {
        component
            .marked_rows()
            .iter()
            .map(|row| row.path.clone())
            .collect()
    }

    fn path(name: &str) -> PathBuf {
        PathBuf::from(format!("/w/{name}"))
    }

    fn dump(component: &mut ListComponent) -> String {
        let mut terminal = Terminal::new(TestBackend::new(70, 8)).unwrap();
        terminal
            .draw(|frame| component.render(frame, frame.area()))
            .unwrap();
        crate::components::buffer_to_string(terminal.backend().buffer())
    }

    fn highlighted_rows(component: &mut ListComponent) -> Vec<u16> {
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
        terminal
            .draw(|frame| component.render(frame, frame.area()))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (1..6)
            .filter(|y| buffer[(2, *y)].style().bg == Some(Color::Blue))
            .collect()
    }

    #[test]
    fn test_marked_paths_is_the_cursor_row_when_no_range_is_open() {
        assert_eq!(marked(&component()), vec![path("one")]);
    }

    #[test]
    fn test_shift_down_extends_the_range_downward() {
        let mut component = component();
        component.handle_event_key(shift_key(KeyCode::Down));
        assert_eq!(marked(&component), vec![path("one"), path("two")]);
    }

    #[test]
    fn test_shift_j_extends_the_range_downward() {
        let mut component = component();
        component.handle_event_key(shift_key(KeyCode::Char('J')));
        assert_eq!(marked(&component), vec![path("one"), path("two")]);
    }

    #[test]
    fn test_shift_up_extends_the_range_upward() {
        let mut component = component();
        component.handle_event_key(key(KeyCode::Char('j')));
        component.handle_event_key(key(KeyCode::Char('j')));
        component.handle_event_key(shift_key(KeyCode::Up));
        assert_eq!(marked(&component), vec![path("two"), path("three")]);
    }

    #[test]
    fn test_shift_k_extends_the_range_upward() {
        let mut component = component();
        component.handle_event_key(key(KeyCode::Char('j')));
        component.handle_event_key(shift_key(KeyCode::Char('K')));
        assert_eq!(marked(&component), vec![path("one"), path("two")]);
    }

    #[test]
    fn test_reversing_the_range_shrinks_it_back() {
        let mut component = component();
        component.handle_event_key(shift_key(KeyCode::Down));
        component.handle_event_key(shift_key(KeyCode::Down));
        component.handle_event_key(shift_key(KeyCode::Up));
        assert_eq!(marked(&component), vec![path("one"), path("two")]);
    }

    #[test]
    fn test_a_plain_move_collapses_the_range() {
        let mut component = component();
        component.handle_event_key(shift_key(KeyCode::Down));
        component.handle_event_key(key(KeyCode::Down));
        assert_eq!(marked(&component), vec![path("three")]);
    }

    #[test]
    fn test_the_range_stops_at_the_last_row() {
        let mut component = component();
        for _ in 0..5 {
            component.handle_event_key(shift_key(KeyCode::Down));
        }
        assert_eq!(
            marked(&component),
            vec![path("one"), path("two"), path("three")]
        );
    }

    #[test]
    fn test_a_query_collapses_the_range() {
        let mut component = component();
        component.handle_event_key(shift_key(KeyCode::Down));
        component.handle_event_key(key(KeyCode::Char('/')));
        component.handle_event_key(key(KeyCode::Char('t')));
        assert_eq!(marked(&component), vec![path("two")]);
    }

    #[test]
    fn test_esc_clears_the_range_before_the_filter() {
        let mut component = component();
        component.handle_event_key(key(KeyCode::Char('/')));
        component.handle_event_key(key(KeyCode::Char('t')));
        component.handle_event_key(key(KeyCode::Enter));
        component.handle_event_key(shift_key(KeyCode::Down));
        assert_eq!(marked(&component), vec![path("two"), path("three")]);

        component.handle_event_key(key(KeyCode::Esc));
        assert_eq!(marked(&component), vec![path("three")], "range goes first");
        assert!(
            !dump(&mut component).contains("one"),
            "filter still applied"
        );

        component.handle_event_key(key(KeyCode::Esc));
        assert_eq!(marked(&component), vec![path("one")], "then the filter");
        assert!(dump(&mut component).contains("one"), "filter is cleared");
    }

    #[test]
    fn test_marked_paths_is_empty_when_no_row_matches() {
        let mut component = component();
        component.handle_event_key(key(KeyCode::Char('/')));
        component.handle_event_key(key(KeyCode::Char('z')));
        assert!(marked(&component).is_empty());
    }

    #[test]
    fn test_d_is_left_for_the_app_to_handle() {
        let mut component = component();
        assert!(matches!(
            component.handle_event_key(key(KeyCode::Char('d'))),
            KeyEventResponse::Ignored
        ));
    }

    #[test]
    fn test_render_inverts_the_cursor_row_instead_of_marking_it() {
        let mut component = component();
        assert_eq!(highlighted_rows(&mut component), vec![1]);
    }

    #[test]
    fn test_an_open_search_bar_takes_the_selection_feedback_off_the_list() {
        let mut component = component();
        component.handle_event_key(key(KeyCode::Char('/')));
        assert!(highlighted_rows(&mut component).is_empty());
    }

    #[test]
    fn test_an_open_search_bar_hides_the_feedback_for_a_range_too() {
        let mut component = component();
        component.handle_event_key(shift_key(KeyCode::Down));
        assert_eq!(highlighted_rows(&mut component), vec![1, 2]);
        component.handle_event_key(key(KeyCode::Char('/')));
        assert!(highlighted_rows(&mut component).is_empty());
    }

    #[test]
    fn test_committing_the_query_gives_the_selection_feedback_back() {
        let mut component = component();
        component.handle_event_key(key(KeyCode::Char('/')));
        component.handle_event_key(key(KeyCode::Enter));
        assert_eq!(highlighted_rows(&mut component), vec![1]);
    }

    #[test]
    fn test_cancelling_the_query_gives_the_selection_feedback_back() {
        let mut component = component();
        component.handle_event_key(key(KeyCode::Char('/')));
        component.handle_event_key(key(KeyCode::Esc));
        assert_eq!(highlighted_rows(&mut component), vec![1]);
    }

    #[test]
    fn test_render_inverts_every_row_of_an_open_range() {
        let mut component = component();
        component.handle_event_key(shift_key(KeyCode::Down));
        assert_eq!(highlighted_rows(&mut component), vec![1, 2]);
    }

    #[test]
    fn test_render_shows_no_cursor_caret() {
        let mut component = component();
        let text = dump(&mut component);
        assert!(!text.contains("> spectra"), "{text}");
    }

    #[test]
    fn test_render_advertises_the_delete_key() {
        let mut component = component();
        let text = dump(&mut component);
        assert!(text.contains("Delete: d"), "{text}");
    }
}
