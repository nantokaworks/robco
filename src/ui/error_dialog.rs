use ratatui::text::{Line, Span};

use super::theme::DEFAULT as THEME;

pub(super) fn content(lines: &[String], can_force: bool) -> Vec<Line<'static>> {
    let mut rendered: Vec<_> = lines
        .iter()
        .map(|line| {
            if line.starts_with("Warning:") {
                Line::from(Span::styled(line.clone(), THEME.dialog_label_style()))
            } else {
                Line::from(line.clone())
            }
        })
        .collect();
    rendered.push(Line::from(""));
    let hint = if can_force {
        "f force delete   any other key dismiss"
    } else {
        "press any key to dismiss"
    };
    rendered.push(Line::from(Span::styled(hint, THEME.hint_style())));
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn force_content_includes_action_hint() {
        let lines = content(&["failed".into()], true);

        assert_eq!(
            lines.last().unwrap().to_string(),
            "f force delete   any other key dismiss"
        );
    }
}
