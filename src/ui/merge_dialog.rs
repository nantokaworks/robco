use ratatui::text::{Line, Span};

use super::{App, spinner, theme::DEFAULT as THEME};

pub(super) fn in_progress(
    app: &App,
    branch: &str,
    step: &'static str,
) -> (&'static str, Vec<Line<'static>>) {
    let indicator = format!("{} {step}", spinner::frame(app.started.elapsed()));
    (
        "merging",
        vec![
            Line::from(branch.to_string()),
            Line::from(Span::styled(indicator, THEME.accent_style())),
        ],
    )
}

pub(super) fn complete(branch: &str) -> (&'static str, Vec<Line<'static>>) {
    (
        "merge complete",
        vec![
            Line::from(branch.to_string()),
            Line::from(Span::styled("press any key", THEME.hint_style())),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_content_names_the_branch() {
        let (title, lines) = complete("feature/test");

        assert_eq!(title, "merge complete");
        assert_eq!(lines[0].to_string(), "feature/test");
        assert_eq!(lines[1].to_string(), "press any key");
    }
}
