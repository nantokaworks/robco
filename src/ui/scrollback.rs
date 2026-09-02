use ansi_to_tui::IntoText;
use ratatui::{layout::Rect, text::Text};

use crate::{
    agent,
    model::{Selection, Status},
    overseer, tmux,
};

use super::{App, PreviewPane, preview::PREVIEW_PADDING};

/// The tmux session mirrored by the current preview tab, when that tab shows a
/// live session (CLAUDE / TERM tabs and orphan previews). `None` for static
/// content (INFO summary, DIFF), whose scrolling stays paragraph-based.
pub(in crate::ui) fn live_session(app: &App) -> Option<String> {
    let prefix = &app.config.tmux_session_prefix;
    match (app.preview, app.selected_item()?) {
        // The row's Info tab mirrors the control session live — see
        // `panes_for`'s comment on `Selection::OverseerAi`.
        (PreviewPane::Info, Selection::OverseerAi) => Some(overseer::control_session_name(prefix)),
        // Mirrors the channel's tmux session live while a turn is running
        // (dropr:371); `None` once the turn ends and the session tears down,
        // which falls back to the "no live session" message `preview::draw`
        // renders for this selection.
        (PreviewPane::Info, Selection::DiscordChannel(index)) => {
            crate::ui::overseer::ordered_channel_ids(&app.overseer_snapshot.discord_channels)
                .get(index)
                .map(|channel_id| overseer::discord_channel_session_name(prefix, channel_id))
        }
        (PreviewPane::Claude, Selection::Repo(repo)) => Some(agent::repo_claude_session_name(
            prefix,
            &app.registry.repos[repo],
        )),
        (PreviewPane::Terminal, Selection::Repo(repo)) => Some(agent::repo_shell_session_name(
            prefix,
            &app.registry.repos[repo],
        )),
        (PreviewPane::Claude, Selection::Agent { repo, agent }) => {
            let agent = &app.registry.repos[repo].agents[agent];
            (agent.status != Status::BranchOnly).then(|| agent.tmux_session.clone())
        }
        (PreviewPane::Terminal, Selection::Agent { repo, agent }) => {
            let agent = &app.registry.repos[repo].agents[agent];
            (agent.status != Status::BranchOnly).then(|| agent::shell_session_name(agent))
        }
        (_, Selection::Orphan(orphan)) => app.orphans.get(orphan).map(|o| o.name.clone()),
        _ => None,
    }
}

/// Inner (content) width and height of a preview pane, after subtracting the
/// border and [`PREVIEW_PADDING`] on each edge. This is the tmux window size a
/// mirrored session is resized to before capture.
pub(in crate::ui) fn inner_dims(area: Rect) -> (u16, u16) {
    (
        area.width.saturating_sub(2 + 2 * PREVIEW_PADDING),
        area.height.saturating_sub(2 + 2 * PREVIEW_PADDING),
    )
}

/// Resize the mirrored session to the given inner size, then capture one
/// screenful `offset` lines back from the live edge. Spawns tmux subprocesses,
/// so it must run off the UI thread (see the `preview_capture` action).
pub(in crate::ui) fn capture_inner(
    session: &str,
    width: u16,
    height: u16,
    offset: u16,
) -> Option<Text<'static>> {
    if width == 0 || height == 0 {
        return None;
    }
    let server = tmux::TmuxServer::default_server();
    let _ = tmux::resize_session(&server, session, width, height);
    tmux::capture_scrollback(&server, session, offset, height)
        .ok()?
        .into_text()
        .ok()
}

impl App {
    /// Scroll the preview one step. Live tmux tabs walk the session's
    /// scrollback history — `preview_scroll` counts lines back from the live
    /// edge, clamped to what the pane's history holds. Static tabs keep
    /// paragraph semantics, counting lines down from the top.
    pub(in crate::ui) fn scroll_preview(&mut self, up: bool, step: u16) {
        self.preview_scroll = match (live_session(self), up) {
            (Some(session), true) => {
                let history = (!self.is_remote_session(&session)).then(|| {
                    tmux::history_size(&tmux::TmuxServer::default_server(), &session).unwrap_or(0)
                });
                tmux_scroll_offset(self.preview_scroll, step, history)
            }
            (Some(_), false) => self.preview_scroll.saturating_sub(step),
            (None, true) => self.preview_scroll.saturating_sub(step),
            (None, false) => self.preview_scroll.saturating_add(step),
        };
    }
}

fn tmux_scroll_offset(current: u16, step: u16, local_history: Option<u16>) -> u16 {
    let offset = current.saturating_add(step);
    local_history.map_or(offset, |limit| offset.min(limit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::Config,
        model::{AgentNode, HostLabel, RepoNode},
        registry::Registry,
        ui::actions::remote_hosts::HostSlot,
    };

    #[test]
    fn remote_scrollback_is_not_clamped_by_local_history() {
        let now = chrono::Local::now();
        let agent = AgentNode {
            id: "worker".into(),
            parent_agent_id: None,
            title: "worker".into(),
            task_number: None,
            worktree_path: "/remote/repo-worker".into(),
            branch: "worker".into(),
            base_commit: String::new(),
            program: "codex".into(),
            spawned_by_version: None,
            claude_session_id: None,
            profile: None,
            tmux_session: "robco_worker".into(),
            created_at: now,
            updated_at: now,
            status: Status::Running,
            worktree_missing: false,
            merge_error: None,
            last_capture: None,
            last_spinner: None,
            last_change_at: None,
            last_auto_accept_at: None,
            shell_working: false,
            mcp_active: false,
            pane_pid: None,
            tracked_command: None,
            subagents: Vec::new(),
            children: Vec::new(),
        };
        let mut repo: RepoNode = serde_json::from_value(serde_json::json!({
            "path": "/remote/repo", "name": "repo", "remote_url": null,
            "pinned": true
        }))
        .unwrap();
        let host = HostLabel {
            name: "Remote".into(),
            ssh: "remote".into(),
        };
        repo.host = Some(host.clone());
        repo.agents = vec![agent];
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
        app.registry.repos = vec![repo];
        app.hosts = vec![HostSlot::idle(host)];
        app.overseer_visible = false;
        app.expanded = vec![true];
        app.selected = app
            .visible()
            .iter()
            .position(|item| matches!(item, Selection::Agent { .. }))
            .unwrap();
        app.preview = PreviewPane::Claude;
        app.preview_scroll = 7;

        app.scroll_preview(true, 5);

        assert_eq!(app.preview_scroll, 12);
    }
}
