use ratatui::style::{Color, Modifier, Style};

pub const BRANCH: &str = "\u{f418}";
pub const WORKTREE: &str = "\u{f07b}";
pub const DIRTY: &str = "✱";
pub const SEARCH: &str = "\u{f002}";
pub const NEW: &str = "\u{f067}";
pub const TRASH: &str = "\u{f1f8}";
pub const WARN: &str = "\u{f071}";
pub const DONE: &str = "\u{f00c}";
pub const FAILED: &str = "\u{f00d}";
pub const CHECKED: &str = "\u{f14a}";
pub const UNCHECKED: &str = "\u{f096}";
pub const FOCUS: &str = "\u{f054}";

pub const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub fn selection() -> Style {
    Style::new().bg(Color::Blue)
}

pub fn accent() -> Style {
    Style::new().fg(Color::Cyan)
}

pub fn title() -> Style {
    accent().add_modifier(Modifier::BOLD)
}

pub fn dirty() -> Style {
    Style::new().fg(Color::Yellow)
}

pub fn danger() -> Style {
    Style::new().fg(Color::Red)
}

pub fn ok() -> Style {
    Style::new().fg(Color::Green)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Span;

    /// Column arithmetic across the screens assumes every glyph is one cell wide.
    #[test]
    fn test_every_glyph_is_single_width() {
        let glyphs = [
            BRANCH, WORKTREE, DIRTY, SEARCH, NEW, TRASH, WARN, DONE, FAILED, CHECKED, UNCHECKED,
            FOCUS,
        ];
        for glyph in glyphs {
            assert_eq!(Span::from(glyph).width(), 1, "{glyph:?} is not one cell");
        }
        for frame in SPINNER {
            assert_eq!(Span::from(frame.to_string()).width(), 1, "{frame:?}");
        }
    }
}
