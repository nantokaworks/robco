use ansi_to_tui::IntoText;
use ratatui::{
    Frame,
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph, Wrap},
};

use crate::{
    agent, git,
    model::{Selection, Status},
    registry::Registry,
    ui::{
        App, PreviewPane, layout, panes_for, scrollback,
        summary::{agent_summary, child_summary, repo_summary},
        theme::DEFAULT as THEME,
    },
};
/// Inner padding between the preview border and its content, applied to every
/// tab. `scrollback::capture` subtracts it when sizing mirrored tmux sessions.
pub(in crate::ui) const PREVIEW_PADDING: u16 = 1;

pub fn draw(frame: &mut Frame<'_>, app: &App, selection: Option<Selection>) {
    let registry = &app.registry;
    let orphans = &app.orphans;
    let pane = app.preview;
    let scroll = app.preview_scroll;
    let tmux_prefix = &app.config.tmux_session_prefix;
    let default_program = &app.config.default_program;

    let ai_label = ai_label(selection, registry, default_program);
    let root = layout::root(frame.area());
    let panes = layout::panes(root.body);

    let (title, text) = match (pane, selection) {
        (PreviewPane::Terminal, Some(Selection::Repo(repo_idx))) => {
            let repo = &registry.repos[repo_idx];
            let title = format!("{} / main", repo.name);
            let session = agent::repo_shell_session_name(tmux_prefix, repo);
            let text = scrollback::capture(&session, panes.preview, scroll).unwrap_or_else(|| {
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
            let text = scrollback::capture(&session, panes.preview, scroll).unwrap_or_else(|| {
                vec![Line::from(Span::styled(
                    "No AI session. Press enter to open one.",
                    THEME.muted_style(),
                ))]
                .into()
            });
            (title, text)
        }
        (_, Some(Selection::Repo(repo_idx))) => repo_summary(
            &registry.repos[repo_idx],
            panes.preview.width.saturating_sub(4),
        ),
        (PreviewPane::Info, Some(Selection::Agent { repo, agent })) => {
            let repo = &registry.repos[repo];
            agent_summary(repo, &repo.agents[agent])
        }
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
            let text = scrollback::capture(&agent.tmux_session, panes.preview, scroll)
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
            let text = scrollback::capture(&session, panes.preview, scroll).unwrap_or_else(|| {
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
        (PreviewPane::Info, Some(Selection::ChildWorktree { repo, agent, child })) => {
            let repo = &registry.repos[repo];
            child_summary(
                repo,
                &repo.agents[agent],
                &repo.agents[agent].children[child],
            )
        }
        (PreviewPane::Diff, Some(Selection::ChildWorktree { repo, agent, child })) => {
            let repo = &registry.repos[repo];
            let agent = &repo.agents[agent];
            let child = &agent.children[child];
            let label = child.branch.as_deref().unwrap_or_else(|| {
                child
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("worktree")
            });
            let text = git::diff(&child.path)
                .unwrap_or_else(|err| err.to_string())
                .into_text()
                .unwrap_or_else(|_| vec![Line::from("Could not render diff.")].into());
            (format!("{} / {} / {label}", repo.name, agent.title), text)
        }
        (_, Some(Selection::Orphan(orphan_idx))) => {
            let Some(orphan) = orphans.get(orphan_idx) else {
                return;
            };
            let text =
                scrollback::capture(&orphan.name, panes.preview, scroll).unwrap_or_else(|| {
                    vec![Line::from(Span::styled(
                        "Session is gone.",
                        THEME.muted_style(),
                    ))]
                    .into()
                });
            (orphan.name.clone(), text)
        }
        // `None` (no repositories) or a pane invalid for the selection.
        _ => (
            "PREVIEW".to_string(),
            vec![Line::from("No repositories discovered.")].into(),
        ),
    };

    // Live tmux tabs already captured the scrolled-back window; scrolling the
    // paragraph on top of that would double-shift. Static tabs keep it.
    let para_scroll = if scrollback::live_session(app).is_some() {
        0
    } else {
        scroll
    };
    let preview = Paragraph::new(text)
        .block(
            Block::default()
                .title_top(preview_tabs_line(pane, selection, &ai_label))
                .title_top(Line::from(title).right_aligned())
                .borders(Borders::ALL)
                .padding(Padding::uniform(PREVIEW_PADDING)),
        )
        .style(THEME.accent_style())
        .wrap(Wrap { trim: false })
        .scroll((para_scroll, 0));
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
                .borders(Borders::ALL)
                .padding(Padding::uniform(PREVIEW_PADDING)),
        )
        .style(THEME.muted_style())
        .wrap(Wrap { trim: false });
    frame.render_widget(preview, area);
}
