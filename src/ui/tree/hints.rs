use ratatui::text::{Line, Span};

use super::{Selection, THEME};

const KEY_HINTS: &[(&str, &str)] = &[
    ("↑↓/jk", "move"),
    ("⇞⇟", "scroll"),
    ("⇥", "pane"),
    ("↵", "attach"),
    ("n", "new"),
    ("r", "restart"),
    ("g", "manage"),
    ("S", "stop"),
    ("R", "reset"),
    ("m", "merge"),
    ("x", "kill"),
    ("?", "help"),
    ("q", "quit"),
];

pub(super) fn r_hint_label(selection: Option<Selection>) -> &'static str {
    if matches!(selection, Some(Selection::Repo(_))) {
        "reload"
    } else {
        "restart"
    }
}

pub(super) fn hints_line(
    message: Option<&str>,
    r_label: &'static str,
    circuit_open: bool,
) -> Line<'static> {
    if let Some(text) = message {
        return Line::from(Span::styled(text.to_string(), THEME.hint_style()));
    }

    let mut spans = Vec::with_capacity(KEY_HINTS.len() * 5);
    for (key, default_label) in KEY_HINTS {
        if *key == "R" && !circuit_open {
            continue;
        }
        let label = if *key == "r" { r_label } else { default_label };
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
    fn reload_hint_is_only_used_for_repo_selection() {
        assert_eq!(r_hint_label(Some(Selection::Repo(0))), "reload");
        assert_eq!(
            r_hint_label(Some(Selection::Agent { repo: 0, agent: 0 })),
            "restart"
        );
        assert_eq!(r_hint_label(None), "restart");
    }

    #[test]
    fn reset_hint_is_only_shown_when_circuit_is_open() {
        assert!(
            hints_line(None, "restart", true)
                .to_string()
                .contains("[R] RESET")
        );
        assert!(
            !hints_line(None, "restart", false)
                .to_string()
                .contains("[R] RESET")
        );
    }
}
