use ratatui::{
    style::Modifier,
    text::{Line, Span, Text},
};

use std::time::{Duration, SystemTime};

use crate::{
    model::{AgentNode, ChildWorktree, RepoNode},
    subagents::SubagentStatus,
};

use super::{blockfont, repo_description, theme::DEFAULT as THEME};

mod dropr_tasks;
use dropr_tasks::dropr_task_lines;

pub(in crate::ui) fn repo_summary(
    repo: &RepoNode,
    repos_root: &std::path::Path,
    width: u16,
) -> (String, Text<'static>) {
    let rendered_name = blockfont::render_fitting(&repo.name, usize::from(width));
    let name_style = if rendered_name.is_some() {
        THEME.accent_style()
    } else {
        THEME.accent_style().add_modifier(Modifier::BOLD)
    };
    let mut lines: Vec<_> = rendered_name
        .unwrap_or_else(|| vec![repo.name.clone()])
        .into_iter()
        .map(|line| Line::from(Span::styled(line, name_style)))
        .collect();

    if let Some(description) = repo_description::get(repo) {
        lines.push(Line::from(Span::styled(description, THEME.muted_style())));
    }
    lines.push(Line::from(""));

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
        Line::from(vec![
            Span::styled("repos root: ", THEME.muted_style()),
            Span::raw(repos_root.display().to_string()),
        ]),
    ]);

    if let Some(dropr) = &repo.dropr {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "─".repeat(usize::from(width)),
            THEME.muted_style(),
        )));
        lines.push(Line::from(Span::styled("DROPR", THEME.accent_style())));
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
        lines.extend(dropr_task_lines(&repo.dropr_tasks));
    }

    (repo.name.clone(), lines.into())
}

pub(in crate::ui) fn agent_summary(repo: &RepoNode, agent: &AgentNode) -> (String, Text<'static>) {
    let field = |name: &str, value: String| {
        Line::from(vec![
            Span::styled(format!("{name}: "), THEME.muted_style()),
            Span::raw(value),
        ])
    };
    let mut lines = vec![
        field("branch", agent.branch.clone()),
        field("worktree", agent.worktree_path.display().to_string()),
        field("status", agent.status.badge().to_string()),
        field(
            "tracked command",
            agent
                .tracked_command
                .clone()
                .unwrap_or_else(|| "(none)".into()),
        ),
        Line::from(""),
        Line::from(Span::styled("subagents", THEME.accent_style())),
    ];
    if agent.subagents.is_empty() {
        lines.push(Line::from(Span::styled(
            "(none active or recent)",
            THEME.muted_style(),
        )));
    } else {
        let now = SystemTime::now();
        for subagent in &agent.subagents {
            let (status, style) = match subagent.status {
                SubagentStatus::Running => ("running", THEME.subagent_style()),
                SubagentStatus::Done => ("done", THEME.muted_style()),
            };
            let elapsed_until = match subagent.status {
                SubagentStatus::Running => now,
                SubagentStatus::Done => subagent.last_activity_at,
            };
            let elapsed = elapsed_until
                .duration_since(subagent.started_at)
                .unwrap_or(Duration::ZERO);
            lines.push(Line::from(vec![
                Span::styled(format!("✻ {}", subagent.agent_type), style),
                Span::styled(
                    format!("  {status}  {}", format_elapsed(elapsed)),
                    THEME.muted_style(),
                ),
            ]));
            lines.push(Line::from(format!("  {}", subagent.description)));
        }
    }
    (format!("{} / {}", repo.name, agent.title), lines.into())
}

fn format_elapsed(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    let minutes = seconds / 60;
    if minutes == 0 {
        format!("{seconds}s")
    } else {
        format!("{minutes}m {:02}s", seconds % 60)
    }
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
