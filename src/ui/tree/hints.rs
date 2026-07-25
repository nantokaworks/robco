use ratatui::text::{Line, Span};

use super::THEME;

/// Only the keys worth a permanent slot in the footer. The complete keymap
/// lives behind `?` (see `crate::ui::help`), so the footer stays an entry
/// point rather than a reference.
const KEY_HINTS: &[(&str, &str)] = &[
    ("↵", "attach"),
    ("n", "new"),
    ("R", "reset"),
    ("m", "merge"),
    ("?", "help"),
    ("q", "quit"),
];

pub(super) fn hints_line(message: Option<&str>, circuit_open: bool) -> Line<'static> {
    if let Some(text) = message {
        return Line::from(Span::styled(text.to_string(), THEME.hint_style()));
    }

    let mut spans = Vec::with_capacity(KEY_HINTS.len() * 5);
    for (key, label) in KEY_HINTS {
        if *key == "R" && !circuit_open {
            continue;
        }
        if !spans.is_empty() {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled("[", THEME.accent_style()));
        spans.push(Span::styled(*key, THEME.accent_bold_style()));
        spans.push(Span::styled("]", THEME.accent_style()));
        spans.push(Span::styled(
            format!(" {}", label.to_uppercase()),
            THEME.hint_style(),
        ));
    }

    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_hint_is_only_shown_when_circuit_is_open() {
        assert!(hints_line(None, true).to_string().contains("[R] RESET"));
        assert!(!hints_line(None, false).to_string().contains("[R] RESET"));
    }

    #[test]
    fn footer_carries_only_the_essential_hints() {
        let line = hints_line(None, false).to_string();
        assert_eq!(line, "[↵] ATTACH [n] NEW [m] MERGE [?] HELP [q] QUIT");
    }
}
