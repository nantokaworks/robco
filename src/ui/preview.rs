use ratatui::{
    Frame,
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph, Wrap},
};

use crate::{
    agent,
    locale::t,
    model::{Selection, Status},
    ui::{
        App, PreviewPane,
        actions::remote_hosts::HostConnection,
        layout, merge_dialog, scrollback,
        summary::{agent_summary, child_summary},
        theme::DEFAULT as THEME,
    },
};

mod agent_details;
mod agent_escalation;
mod branch_only;
mod dropr_task_preview;
#[cfg(test)]
mod error_info_tests;
mod labels;
mod notice;
mod overseer;
mod remote_chat;
#[cfg(test)]
mod render_tests;
pub(in crate::ui) mod tabs;
use labels::ai_label;
use notice::render_merge_notice;
use tabs::preview_tabs_line;
pub(in crate::ui) const PREVIEW_PADDING: u16 = 1;
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
        (_, Some(Selection::OverseerCategory(category))) => {
            super::overseer::category_preview(app, category)
        }
        (_, Some(Selection::OverseerAi)) => overseer::control_preview(app),
        (
            _,
            Some(Selection::OverseerAlert(index) | Selection::RepoEscalation { item: index, .. }),
        ) => {
            let Some(item) = app.overseer_inbox.get(index) else {
                return;
            };
            (
                format!("[{}] {}", item.kind.code(), item.target_id),
                agent_escalation::item_lines(app, item).into(),
            )
        }
        (_, Some(Selection::DiscordChannel(index))) => {
            let Some(preview) = remote_chat::render_local_discord(app, index) else {
                return;
            };
            preview
        }
        (_, Some(Selection::RemoteHostError(host))) => {
            let (Some(slot), Some(view)) = (app.hosts.get(host), app.host_view(host)) else {
                return;
            };
            let state = match view.connection {
                HostConnection::Connecting => "connecting",
                HostConnection::Connected => "connected",
                HostConnection::Failed => "failed",
            };
            let error = view
                .error
                .as_deref()
                .unwrap_or_default()
                .replace('\n', "\n       ");
            let text = format!(
                "host: {}\nssh: {}\nconnection: {state}\nerror: {error}",
                slot.label.name, slot.label.ssh
            );
            (slot.label.name.clone(), text.into())
        }
        (
            _,
            Some(
                selection
                @ (Selection::RemoteControlAi(_) | Selection::RemoteDiscordChannel { .. }),
            ),
        ) => {
            let Some(preview) = remote_chat::render(app, selection) else {
                return;
            };
            preview
        }
        (PreviewPane::Terminal, Some(Selection::Repo(repo_idx))) => {
            let repo = &registry.repos[repo_idx];
            let title = format!("{} / main", repo.name);
            let session = agent::repo_shell_session_name(tmux_prefix, repo);
            let text = app.cached_tmux(&session).unwrap_or_else(|| {
                vec![Line::from(Span::styled(
                    t(app.locale, "No shell session. Press enter to open one."),
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
                    t(app.locale, "No AI session. Press enter to open one."),
                    THEME.muted_style(),
                ))]
                .into()
            });
            (title, text)
        }
        (_, Some(Selection::Repo(repo_idx))) => {
            let repo = &registry.repos[repo_idx];
            let dropr_fetch_in_flight = repo
                .dropr
                .as_ref()
                .is_some_and(|workspace| app.dropr_task_fetch_running(&workspace.id));
            dropr_task_preview::render(
                repo,
                &app.config.repos_root,
                &app.overseer_snapshot.ledger,
                &app.overseer_snapshot.other_prs,
                panes.preview.width.saturating_sub(4),
                app.locale,
                app.dropr_task_focus,
                dropr_fetch_in_flight,
            )
        }
        (PreviewPane::Error, Some(Selection::Agent { repo, agent })) => {
            let repo = &registry.repos[repo];
            let agent = &repo.agents[agent];
            let title = format!("{} / {}", repo.name, agent.title);
            (title, merge_dialog::error_lines(app, selection).into())
        }
        (PreviewPane::Info, Some(Selection::Agent { repo, agent })) => {
            let repo = &registry.repos[repo];
            let agent = &repo.agents[agent];
            let (title, mut text) = agent_summary(repo, agent, app.locale);
            text.lines.splice(3..3, agent_details::lines(app, agent));
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
                    Line::from(Span::styled(
                        t(app.locale, "No preview available."),
                        THEME.muted_style(),
                    )),
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
                    t(app.locale, "No shell session. Press enter to open one."),
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
            let text =
                super::preview_pane::worktree_diff(app, repo.host.is_some(), &agent.worktree_path);
            (title, text)
        }
        (PreviewPane::Info, Some(Selection::ChildWorktree { repo, agent, child })) => {
            let repo = &registry.repos[repo];
            child_summary(
                repo,
                &repo.agents[agent],
                &repo.agents[agent].children[child],
                app.locale,
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
            let text = super::preview_pane::worktree_diff(app, repo.host.is_some(), &child.path);
            (format!("{} / {} / {label}", repo.name, agent.title), text)
        }
        (_, Some(Selection::Orphan(orphan_idx))) => {
            let Some(orphan) = orphans.get(orphan_idx) else {
                return;
            };
            let text = app.cached_tmux(&orphan.name).unwrap_or_else(|| {
                vec![Line::from(Span::styled(
                    t(app.locale, "Session is gone."),
                    THEME.muted_style(),
                ))]
                .into()
            });
            (orphan.name.clone(), text)
        }
        // `None` (no repositories) or a pane invalid for the selection.
        _ => (
            "PREVIEW".to_string(),
            vec![Line::from(t(app.locale, "No repositories discovered."))].into(),
        ),
    };
    // Live tmux tabs already captured the scrolled-back window.
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
