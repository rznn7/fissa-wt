use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};

use crate::config::Icons;

pub struct IconSet {
    pub branch: &'static str,
    pub worktree: &'static str,
    pub dirty: &'static str,
    pub search: &'static str,
    pub new: &'static str,
    pub trash: &'static str,
    pub warn: &'static str,
    pub done: &'static str,
    pub failed: &'static str,
    pub checked: &'static str,
    pub unchecked: &'static str,
    pub focus: &'static str,
    pub spinner: &'static [char],
}

const NERD: IconSet = IconSet {
    branch: "\u{f418}",
    worktree: "\u{f07b}",
    dirty: "\u{2731}",
    search: "\u{f002}",
    new: "\u{f067}",
    trash: "\u{f1f8}",
    warn: "\u{f071}",
    done: "\u{f00c}",
    failed: "\u{f00d}",
    checked: "\u{f14a}",
    unchecked: "\u{f096}",
    focus: "\u{f054}",
    spinner: &[
        '\u{280b}', '\u{2819}', '\u{2839}', '\u{2838}', '\u{283c}', '\u{2834}', '\u{2826}',
        '\u{2827}', '\u{2807}', '\u{280f}',
    ],
};

const ASCII: IconSet = IconSet {
    branch: "#",
    worktree: "/",
    dirty: "*",
    search: "?",
    new: "+",
    trash: "X",
    warn: "!",
    done: "v",
    failed: "x",
    checked: "x",
    unchecked: "-",
    focus: ">",
    spinner: &['|', '/', '-', '\\'],
};

static ICONS: OnceLock<&'static IconSet> = OnceLock::new();

pub fn icon_set(icons: Icons) -> &'static IconSet {
    match icons {
        Icons::Nerd => &NERD,
        Icons::Ascii => &ASCII,
    }
}

/// Called once before the TUI starts; every later read sees the same set.
pub fn install(icons: Icons) {
    let _ = ICONS.set(icon_set(icons));
}

fn active() -> &'static IconSet {
    ICONS.get().copied().unwrap_or(&NERD)
}

pub fn branch() -> &'static str {
    active().branch
}

pub fn worktree() -> &'static str {
    active().worktree
}

pub fn dirty_icon() -> &'static str {
    active().dirty
}

pub fn search() -> &'static str {
    active().search
}

pub fn new() -> &'static str {
    active().new
}

pub fn trash() -> &'static str {
    active().trash
}

pub fn warn() -> &'static str {
    active().warn
}

pub fn done() -> &'static str {
    active().done
}

pub fn failed() -> &'static str {
    active().failed
}

pub fn checked() -> &'static str {
    active().checked
}

pub fn unchecked() -> &'static str {
    active().unchecked
}

pub fn focus() -> &'static str {
    active().focus
}

pub fn spinner() -> &'static [char] {
    active().spinner
}

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
    fn test_every_glyph_of_every_set_is_single_width() {
        for set in [icon_set(Icons::Nerd), icon_set(Icons::Ascii)] {
            let glyphs = [
                set.branch,
                set.worktree,
                set.dirty,
                set.search,
                set.new,
                set.trash,
                set.warn,
                set.done,
                set.failed,
                set.checked,
                set.unchecked,
                set.focus,
            ];
            for glyph in glyphs {
                assert_eq!(Span::from(glyph).width(), 1, "{glyph:?} is not one cell");
            }
            for frame in set.spinner {
                assert_eq!(Span::from(frame.to_string()).width(), 1, "{frame:?}");
            }
        }
    }

    #[test]
    fn test_the_ascii_set_is_free_of_nerd_font_glyphs() {
        let set = icon_set(Icons::Ascii);
        let glyphs = [
            set.branch,
            set.worktree,
            set.dirty,
            set.search,
            set.new,
            set.trash,
            set.warn,
            set.done,
            set.failed,
            set.checked,
            set.unchecked,
            set.focus,
        ];
        for glyph in glyphs {
            assert!(glyph.is_ascii(), "{glyph:?} is not ascii");
        }
        assert!(set.spinner.iter().all(char::is_ascii), "{:?}", set.spinner);
    }

    #[test]
    fn test_a_checkbox_pair_is_visibly_different_in_both_sets() {
        for set in [icon_set(Icons::Nerd), icon_set(Icons::Ascii)] {
            assert_ne!(set.checked, set.unchecked);
        }
    }

    #[test]
    fn test_the_glyphs_default_to_the_nerd_set() {
        assert_eq!(branch(), icon_set(Icons::Nerd).branch);
    }
}
