use ansi_to_tui::IntoText;
use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::{
    agent, git,
    model::{Selection, Status},
    registry::Registry,
    tmux,
    ui::{PreviewPane, layout, theme::DEFAULT as THEME},
};

pub fn draw(
    frame: &mut Frame<'_>,
    selection: Option<Selection>,
    registry: &Registry,
    pane: PreviewPane,
    scroll: u16,
) {
    let root = layout::root(frame.area());
    let panes = layout::panes(root.body);

    let (title, text) = match (pane, selection) {
        (_, Some(Selection::Repo(repo_idx))) => repo_summary(&registry.repos[repo_idx]),
        (PreviewPane::Claude, Some(Selection::Agent { repo, agent })) => {
            let repo = &registry.repos[repo];
            let agent = &repo.agents[agent];
            let title = format!("CLAUDE: {} / {}", repo.name, agent.title);
            if agent.status == Status::BranchOnly {
                return render_branch_only(frame, panes.preview, title, &agent.branch);
            }
            resize_preview_session(&agent.tmux_session, panes.preview);
            let text = tmux::capture_plain(&agent.tmux_session)
                .ok()
                .and_then(|capture| capture.into_text().ok())
                .unwrap_or_else(|| {
                    vec![
                        Line::from(Span::styled("No preview available.", THEME.muted_style())),
                        Line::from(Span::styled(&agent.tmux_session, THEME.muted_style())),
                    ]
                    .into()
                });
            (title, text)
        }
        (PreviewPane::Terminal, Some(Selection::Agent { repo, agent })) => {
            let repo = &registry.repos[repo];
            let agent = &repo.agents[agent];
            let title = format!("TERMINAL: {} / {}", repo.name, agent.title);
            if agent.status == Status::BranchOnly {
                return render_branch_only(frame, panes.preview, title, &agent.branch);
            }
            let session = agent::shell_session_name(agent);
            resize_preview_session(&session, panes.preview);
            let text = tmux::capture_plain(&session)
                .ok()
                .and_then(|capture| capture.into_text().ok())
                .unwrap_or_else(|| {
                    vec![Line::from(Span::styled(
                        "No shell session. Press enter to open one.",
                        THEME.muted_style(),
                    ))]
                    .into()
                });
            (title, text)
        }
        (PreviewPane::Diff, Some(Selection::Agent { repo, agent })) => {
            let repo = &registry.repos[repo];
            let agent = &repo.agents[agent];
            let title = format!("DIFF: {} / {}", repo.name, agent.title);
            if agent.status == Status::BranchOnly {
                return render_branch_only(frame, panes.preview, title, &agent.branch);
            }
            let text = git::diff(&agent.worktree_path)
                .unwrap_or_else(|err| err.to_string())
                .into_text()
                .unwrap_or_else(|_| vec![Line::from("Could not render diff.")].into());
            (title, text)
        }
        (_, None) => (
            "PREVIEW".to_string(),
            vec![Line::from("No repositories discovered.")].into(),
        ),
    };

    let preview = Paragraph::new(text)
        .block(
            Block::default()
                .title(title)
                .title_style(Style::default().add_modifier(Modifier::BOLD))
                .borders(Borders::ALL),
        )
        .style(THEME.accent_style())
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(preview, panes.preview);
}

fn resize_preview_session(session: &str, area: ratatui::layout::Rect) {
    let width = area.width.saturating_sub(2);
    let height = area.height.saturating_sub(2);
    if width == 0 || height == 0 {
        return;
    }
    let _ = tmux::resize_session(session, width, height);
}

fn render_branch_only(
    frame: &mut Frame<'_>,
    area: ratatui::layout::Rect,
    title: String,
    branch: &str,
) {
    let text = vec![
        Line::from(Span::styled(
            "Worktree has been removed.",
            THEME.muted_style(),
        )),
        Line::from(vec![
            Span::styled("branch: ", THEME.muted_style()),
            Span::raw(branch.to_string()),
        ]),
        Line::from(Span::styled(
            "Press x to delete the branch.",
            THEME.muted_style(),
        )),
    ];
    let preview = Paragraph::new(text)
        .block(
            Block::default()
                .title(title)
                .title_style(Style::default().add_modifier(Modifier::BOLD))
                .borders(Borders::ALL),
        )
        .style(THEME.muted_style())
        .wrap(Wrap { trim: false });
    frame.render_widget(preview, area);
}

fn repo_summary(repo: &crate::model::RepoNode) -> (String, ratatui::text::Text<'static>) {
    let mut lines = vec![
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
    ];

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

    (format!("REPO: {}", repo.name), lines.into())
}
