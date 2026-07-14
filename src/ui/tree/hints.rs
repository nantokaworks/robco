use ratatui::text::{Line, Span};

use super::{Selection, THEME};

const KEY_HINTS: &[(&str, &str)] = &[
    ("↑↓/jk", "move"),
    ("⇞⇟", "scroll"),
    ("⇥", "pane"),
    ("↵", "attach"),
    ("n/N", "new"),
    ("r", "restart"),
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

pub(super) fn hints_line(message: Option<&str>, r_label: &'static str) -> Line<'static> {
    if let Some(text) = message {
        return Line::from(Span::styled(text.to_string(), THEME.hint_style()));
    }

    let mut spans = Vec::with_capacity(KEY_HINTS.len() * 5);
    for (idx, (key, default_label)) in KEY_HINTS.iter().enumerate() {
        let label = if *key == "r" { r_label } else { default_label };
        if idx > 0 {
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
}
