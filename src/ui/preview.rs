use ratatui::{
    Frame,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Padding, Paragraph, Wrap},
};

use crate::{
    agent,
    model::{Selection, Status},
    ui::{
        App, PreviewPane, layout, merge_dialog, scrollback,
        summary::{agent_summary, child_summary, repo_summary},
        theme::DEFAULT as THEME,
    },
};

mod branch_only;
mod labels;
mod notice;
mod overseer;
#[cfg(test)]
mod render_tests;
pub(in crate::ui) mod tabs;
use labels::ai_label;
use notice::render_merge_notice;
use tabs::preview_tabs_line;
/// Inner padding between the preview border and its content, applied to every
/// tab. `scrollback::inner_dims` subtracts it when sizing mirrored tmux sessions.
pub(in crate::ui) const PREVIEW_PADDING: u16 = 1;
/// Height of the preview block's top border, which doubles as the tab bar.
/// Overlays drawn inside the preview start below it so the tabs stay visible.
const TAB_BAR_ROWS: u16 = 1;

pub fn draw(frame: &mut Frame<'_>, app: &App, selection: Option<Selection>) {
    let registry = &app.registry;
    let orphans = &app.orphans;
    let pane = app.preview;
    let scroll = app.preview_scroll;
    let tmux_prefix = &app.config.tmux_session_prefix;
    let default_program = &app.config.default_program;

    let ai_label = ai_label(selection, registry, default_program);
    let root = layout::root(frame.area());
    let panes = layout::panes(root.body, app.overseer_frame_height());

    let (title, text) = match (pane, selection) {
        (PreviewPane::Info, Some(Selection::OverseerCategory(category))) => {
            super::overseer::category_preview(app, category)
        }
        (PreviewPane::Claude, Some(Selection::OverseerCategory(_))) => {
            overseer::control_preview(app)
        }
        // An item row previews itself, not the list it belongs to: the other
        // items are already on screen in the left frame. `panes_for` gives this
        // selection a single Info tab, so the catch-all pane is that one tab.
        (_, Some(Selection::OverseerInbox(index))) => {
            super::overseer::inbox_item_preview(app, index)
        }
        (PreviewPane::Terminal, Some(Selection::Repo(repo_idx))) => {
            let repo = &registry.repos[repo_idx];
            let title = format!("{} / main", repo.name);
            let session = agent::repo_shell_session_name(tmux_prefix, repo);
            let text = app.cached_tmux(&session).unwrap_or_else(|| {
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
            let text = app.cached_tmux(&session).unwrap_or_else(|| {
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
            &app.config.repos_root,
            // The snapshot the OVERSEER frame already refreshes: one ledger for
            // the whole TUI, and no disk read in the render path.
            &app.overseer_snapshot.ledger,
            panes.preview.width.saturating_sub(4),
        ),
        // Rendered as a tab rather than an overlay so reading the failure never
        // costs the operator sight of the tab bar.
        (PreviewPane::Error, Some(Selection::Agent { repo, agent })) => {
            let repo = &registry.repos[repo];
            let agent = &repo.agents[agent];
            let title = format!("{} / {}", repo.name, agent.title);
            (title, merge_dialog::error_lines(app, selection).into())
        }
        (PreviewPane::Info, Some(Selection::Agent { repo, agent })) => {
            let repo = &registry.repos[repo];
            let agent = &repo.agents[agent];
            let (title, mut text) = agent_summary(repo, agent);
            let mut details = Vec::new();
            if agent.worktree_missing {
                details.push(Line::from(vec![
                    Span::styled("worktree missing: ", THEME.muted_style()),
                    Span::styled(
                        agent.worktree_path.display().to_string(),
                        THEME.worktree_missing_style(false),
                    ),
                ]));
            }
            if let Some(error) = &agent.merge_error {
                for (row, error_line) in error.split('\n').enumerate() {
                    details.push(Line::from(vec![
                        Span::styled(
                            if row == 0 {
                                "merge failed: "
                            } else {
                                "              "
                            },
                            THEME.muted_style(),
                        ),
                        Span::styled(error_line.to_string(), THEME.merge_failed_style(false)),
                    ]));
                }
            }
            text.lines.splice(3..3, details);
            (title, text)
        }
        (PreviewPane::Claude, Some(Selection::Agent { repo, agent })) => {
            let selection = Some(Selection::Agent { repo, agent });
            let repo = &registry.repos[repo];
            let agent = &repo.agents[agent];
            let title = format!("{} / {}", repo.name, agent.title);
            if agent.status == Status::BranchOnly {
                return branch_only::render(
                    frame,
                    panes.preview,
                    (app, pane, selection),
                    title,
                    &agent.branch,
                    &ai_label,
                );
            }
            let text = app.cached_tmux(&agent.tmux_session).unwrap_or_else(|| {
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
                return branch_only::render(
                    frame,
                    panes.preview,
                    (app, pane, selection),
                    title,
                    &agent.branch,
                    &ai_label,
                );
            }
            let session = agent::shell_session_name(agent);
            let text = app.cached_tmux(&session).unwrap_or_else(|| {
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
                return branch_only::render(
                    frame,
                    panes.preview,
                    (app, pane, selection),
                    title,
                    &agent.branch,
                    &ai_label,
                );
            }
            let text = app
                .cached_diff(&agent.worktree_path)
                .unwrap_or_else(loading_diff);
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
            let text = app.cached_diff(&child.path).unwrap_or_else(loading_diff);
            (format!("{} / {} / {label}", repo.name, agent.title), text)
        }
        (_, Some(Selection::Orphan(orphan_idx))) => {
            let Some(orphan) = orphans.get(orphan_idx) else {
                return;
            };
            let text = app.cached_tmux(&orphan.name).unwrap_or_else(|| {
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
    let mut block = Block::default()
        .title_top(preview_tabs_line(
            pane,
            &app.preview_panes(selection),
            &ai_label,
        ))
        .title_top(Line::from(title).right_aligned())
        .borders(Borders::ALL)
        .padding(Padding::uniform(PREVIEW_PADDING));
    if let Some(title) = merge_dialog::preview_title(app, selection) {
        block = block.title_bottom(title);
    }
    let preview = Paragraph::new(text)
        .block(block)
        .style(THEME.accent_style())
        .wrap(Wrap { trim: false })
        .scroll((para_scroll, 0));
    frame.render_widget(preview, panes.preview);
    render_merge_notice(frame, app, selection, panes.preview);
}

/// Placeholder shown while a worktree diff is still being captured off-thread.
fn loading_diff() -> Text<'static> {
    vec![Line::from(Span::styled(
        "Loading diff…",
        THEME.muted_style(),
    ))]
    .into()
}
