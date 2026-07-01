use ansi_to_tui::IntoText;
use ratatui::{
    Frame,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::{
    agent, git,
    model::{Selection, Status},
    registry::Registry,
    tmux,
    ui::{PreviewPane, layout, panes_for, theme::DEFAULT as THEME},
};

pub fn draw(
    frame: &mut Frame<'_>,
    selection: Option<Selection>,
    registry: &Registry,
    pane: PreviewPane,
    scroll: u16,
    tmux_prefix: &str,
    default_program: &str,
) {
    let ai_label = ai_label(selection, registry, default_program);
    let root = layout::root(frame.area());
    let panes = layout::panes(root.body);

    let (title, text) = match (pane, selection) {
        (PreviewPane::Terminal, Some(Selection::Repo(repo_idx))) => {
            let repo = &registry.repos[repo_idx];
            let title = format!("{} / main", repo.name);
            let session = agent::repo_shell_session_name(tmux_prefix, repo);
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
        (PreviewPane::Claude, Some(Selection::Repo(repo_idx))) => {
            let repo = &registry.repos[repo_idx];
            let title = format!("{} / main", repo.name);
            let session = agent::repo_claude_session_name(tmux_prefix, repo);
            resize_preview_session(&session, panes.preview);
            let text = tmux::capture_plain(&session)
                .ok()
                .and_then(|capture| capture.into_text().ok())
                .unwrap_or_else(|| {
                    vec![Line::from(Span::styled(
                        "No AI session. Press enter to open one.",
                        THEME.muted_style(),
                    ))]
                    .into()
                });
            (title, text)
        }
        (_, Some(Selection::Repo(repo_idx))) => repo_summary(&registry.repos[repo_idx]),
        (PreviewPane::Claude, Some(Selection::Agent { repo, agent })) => {
            let selection = Some(Selection::Agent { repo, agent });
            let repo = &registry.repos[repo];
            let agent = &repo.agents[agent];
            let title = format!("{} / {}", repo.name, agent.title);
            if agent.status == Status::BranchOnly {
                return render_branch_only(
                    frame,
                    panes.preview,
                    pane,
                    selection,
                    title,
                    &agent.branch,
                    &ai_label,
                );
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
            let selection = Some(Selection::Agent { repo, agent });
            let repo = &registry.repos[repo];
            let agent = &repo.agents[agent];
            let title = format!("{} / {}", repo.name, agent.title);
            if agent.status == Status::BranchOnly {
                return render_branch_only(
                    frame,
                    panes.preview,
                    pane,
                    selection,
                    title,
                    &agent.branch,
                    &ai_label,
                );
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
            let selection = Some(Selection::Agent { repo, agent });
            let repo = &registry.repos[repo];
            let agent = &repo.agents[agent];
            let title = format!("{} / {}", repo.name, agent.title);
            if agent.status == Status::BranchOnly {
                return render_branch_only(
                    frame,
                    panes.preview,
                    pane,
                    selection,
                    title,
                    &agent.branch,
                    &ai_label,
                );
            }
            let text = git::diff(&agent.worktree_path)
                .unwrap_or_else(|err| err.to_string())
                .into_text()
                .unwrap_or_else(|_| vec![Line::from("Could not render diff.")].into());
            (title, text)
        }
        // `None` (no repositories) and any pane not valid for the current
        // selection (e.g. `Info` on an agent, which `restore_preview` prevents
        // from ever becoming active).
        _ => (
            "PREVIEW".to_string(),
            vec![Line::from("No repositories discovered.")].into(),
        ),
    };

    let preview = Paragraph::new(text)
        .block(
            Block::default()
                .title_top(preview_tabs_line(pane, selection, &ai_label))
                .title_top(Line::from(title).right_aligned())
                .borders(Borders::ALL),
        )
        .style(THEME.accent_style())
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(preview, panes.preview);
}

/// Resolve the label shown on the AI tab. Agents surface their own program
/// (profile name, or the first token of the launch command); the main worktree
/// falls back to the configured default program, which is known even when no AI
/// session has been launched yet.
fn ai_label(selection: Option<Selection>, registry: &Registry, default_program: &str) -> String {
    let raw = match selection {
        Some(Selection::Agent { repo, agent }) => {
            let agent = &registry.repos[repo].agents[agent];
            agent.profile.clone().unwrap_or_else(|| {
                agent
                    .program
                    .split_whitespace()
                    .next()
                    .unwrap_or("AI")
                    .to_string()
            })
        }
        _ => default_program.to_string(),
    };
    raw.to_uppercase()
}

fn preview_tabs_line(
    active: PreviewPane,
    selection: Option<Selection>,
    ai_label: &str,
) -> Line<'static> {
    let mut spans = Vec::new();
    for (idx, pane) in panes_for(selection).iter().enumerate() {
        if idx > 0 {
            spans.push(Span::styled(" │ ", THEME.muted_style()));
        }

        let label = match pane {
            PreviewPane::Info => "INFO",
            PreviewPane::Claude => ai_label,
            PreviewPane::Diff => "DIFF",
            PreviewPane::Terminal => "TERM",
        };
        let is_active = *pane == active;
        let text = if is_active {
            format!("[{label}]")
        } else {
            format!(" {label} ")
        };
        let style = if is_active {
            THEME.selection_style()
        } else {
            THEME.muted_style()
        };
        spans.push(Span::styled(text, style));
    }

    Line::from(spans)
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
    active: PreviewPane,
    selection: Option<Selection>,
    title: String,
    branch: &str,
    ai_label: &str,
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
                .title_top(preview_tabs_line(active, selection, ai_label))
                .title_top(Line::from(title).right_aligned())
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

    (repo.name.clone(), lines.into())
}
