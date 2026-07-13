use ratatui::text::{Line, Span, Text};

use crate::model::{AgentNode, ChildWorktree, RepoNode};

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

pub(in crate::ui) fn child_summary(
    repo: &RepoNode,
    agent: &AgentNode,
    child: &ChildWorktree,
) -> (String, Text<'static>) {
    let unknown = || "(unknown)".to_string();
    let field = |name: &str, value: String| {
        Line::from(vec![
            Span::styled(format!("{name}: "), THEME.muted_style()),
            Span::raw(value),
        ])
    };
    let label = child.branch.clone().unwrap_or_else(|| {
        child
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("worktree")
            .to_string()
    });
    let lines = vec![
        field("worktree path", child.path.display().to_string()),
        field(
            "branch",
            child.branch.clone().unwrap_or_else(|| "(detached)".into()),
        ),
        field(
            "HEAD commit",
            child
                .head
                .as_deref()
                .map(|h| h.chars().take(12).collect())
                .unwrap_or_else(unknown),
        ),
        field(
            "state",
            child
                .clean
                .map(|clean| if clean { "clean" } else { "dirty" }.into())
                .unwrap_or_else(unknown),
        ),
        field(
            &format!("ahead/behind vs {}", agent.branch),
            child
                .ahead_behind
                .map(|(behind, ahead)| format!("+{ahead}/-{behind}"))
                .unwrap_or_else(unknown),
        ),
        field("parent agent", format!("{} ({})", agent.title, agent.id)),
        field("ownership signal", "nested under agent worktree".into()),
        field(
            "tmux session",
            child
                .tmux_session
                .clone()
                .unwrap_or_else(|| "(none)".into()),
        ),
        field(
            "last change",
            child
                .modified_at
                .map(|time| time.to_rfc3339())
                .unwrap_or_else(unknown),
        ),
    ];
    (
        format!("{} / {} / {label}", repo.name, agent.title),
        lines.into(),
    )
}
