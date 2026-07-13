use ratatui::text::{Line, Span, Text};

use crate::model::RepoNode;

use super::{
    logo::{ROBCO_FLAVOR, ROBCO_LOGO},
    theme::DEFAULT as THEME,
};

pub(in crate::ui) fn repo_summary(repo: &RepoNode) -> (String, Text<'static>) {
    let mut lines: Vec<_> = ROBCO_LOGO
        .iter()
        .map(|line| Line::from(Span::styled(*line, THEME.accent_style())))
        .chain(
            ROBCO_FLAVOR
                .iter()
                .map(|line| Line::from(Span::styled(*line, THEME.muted_style()))),
        )
        .chain([Line::from("")])
        .collect();

    lines.extend([
        Line::from(vec![
            Span::styled("path: ", THEME.muted_style()),
            Span::raw(repo.path.display().to_string()),
        ]),
        Line::from(vec![
            Span::styled("remote: ", THEME.muted_style()),
            Span::raw(
                repo.remote_url
                    .clone()
                    .unwrap_or_else(|| "(none)".to_string()),
            ),
        ]),
        Line::from(vec![
            Span::styled("agents: ", THEME.muted_style()),
            Span::raw(repo.agents.len().to_string()),
        ]),
    ]);

    if let Some(dropr) = &repo.dropr {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("dropr", THEME.accent_style())));
        lines.push(Line::from(vec![
            Span::styled("kind: ", THEME.muted_style()),
            Span::raw(dropr.kind.clone()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("id: ", THEME.muted_style()),
            Span::raw(dropr.id.clone()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("name: ", THEME.muted_style()),
            Span::raw(dropr.name.clone()),
        ]));
    }

    (repo.name.clone(), lines.into())
}
