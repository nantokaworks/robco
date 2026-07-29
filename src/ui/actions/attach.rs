use crate::{
    Result, agent,
    model::{Selection, Status},
    overseer, tmux,
};

use super::{
    super::{App, suspend_terminal},
    dropr_tasks::DroprTaskReload,
};

impl App {
    /// Suspend the TUI and hand the terminal to a tmux session. A failure here
    /// (e.g. the session exited between `ensure_*` and attach, or a transient
    /// tmux error) is surfaced as an in-app message and MUST NOT propagate: a
    /// bubbling `Err` would unwind out of the event loop and exit robco, which
    /// over ssh drops the whole connection.
    fn attach_session(&mut self, session: &str) {
        self.force_redraw = true;
        if let Err(err) = suspend_terminal(|| tmux::attach(session)) {
            self.show_message(err.to_string());
        }
    }

    pub(in crate::ui) fn attach_selected(&mut self) -> Result<()> {
        if let Some(Selection::ChildWorktree { repo, agent, child }) = self.selected_item() {
            let session = self.registry.repos[repo].agents[agent].children[child]
                .tmux_session
                .clone();
            if let Some(session) = session {
                self.attach_session(&session);
            } else {
                self.show_message("no live session in this child worktree");
            }
            return Ok(());
        }
        let Some(Selection::Agent {
            repo,
            agent: agent_idx,
        }) = self.selected_item()
        else {
            return Ok(());
        };
        let selected = self.registry.repos[repo].agents[agent_idx].clone();
        if selected.status == Status::BranchOnly {
            self.show_message(format!("branch remains: {}", selected.branch));
            return Ok(());
        }
        match agent::ensure_agent_session(&selected) {
            Ok(()) => self.attach_session(&selected.tmux_session),
            Err(err) => self.show_message(err.to_string()),
        }
        Ok(())
    }

    pub(in crate::ui) fn attach_shell_selected(&mut self) -> Result<()> {
        match self.selected_item() {
            Some(Selection::Agent {
                repo,
                agent: agent_idx,
            }) => {
                let selected = self.registry.repos[repo].agents[agent_idx].clone();
                if selected.status == Status::BranchOnly {
                    self.show_message(format!("branch remains: {}", selected.branch));
                    return Ok(());
                }
                match agent::ensure_shell_session(&selected) {
                    Ok(()) => self.attach_session(&agent::shell_session_name(&selected)),
                    Err(err) => self.show_message(err.to_string()),
                }
            }
            Some(Selection::Repo(repo)) => {
                let repo_node = self.registry.repos[repo].clone();
                let prefix = self.config.tmux_session_prefix.clone();
                match agent::ensure_repo_shell_session(&prefix, &repo_node) {
                    Ok(()) => {
                        self.attach_session(&agent::repo_shell_session_name(&prefix, &repo_node))
                    }
                    Err(err) => self.show_message(err.to_string()),
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Attach the Overseer's own control session, creating it when absent.
    /// This is the row's Enter action (see `Selection::OverseerAi`); unlike
    /// `attach_claude_selected` it is not gated on the current preview tab,
    /// since the row's one tab is Info, not Claude.
    pub(in crate::ui) fn attach_control_selected(&mut self) {
        if !matches!(self.selected_item(), Some(Selection::OverseerAi)) {
            return;
        }
        let cwd = self
            .ephemeral_root
            .clone()
            .unwrap_or_else(|| self.config.repos_root.clone());
        match overseer::ensure_control_session(&self.config, &cwd) {
            Ok(session) => self.attach_session(&session),
            Err(err) => self.show_message(err.to_string()),
        }
    }

    pub(in crate::ui) fn attach_claude_selected(&mut self) -> Result<()> {
        match self.selected_item() {
            Some(Selection::Agent {
                repo,
                agent: agent_idx,
            }) => {
                let selected = self.registry.repos[repo].agents[agent_idx].clone();
                if selected.status == Status::BranchOnly {
                    self.show_message(format!("branch remains: {}", selected.branch));
                    return Ok(());
                }
                match agent::ensure_agent_session(&selected) {
                    Ok(()) => self.attach_session(&selected.tmux_session),
                    Err(err) => self.show_message(err.to_string()),
                }
            }
            Some(Selection::Repo(repo)) => {
                let repo_node = self.registry.repos[repo].clone();
                let prefix = self.config.tmux_session_prefix.clone();
                match agent::ensure_repo_claude_session(&self.config, &prefix, &repo_node) {
                    Ok(()) => {
                        self.attach_session(&agent::repo_claude_session_name(&prefix, &repo_node))
                    }
                    Err(err) => self.show_message(err.to_string()),
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(in crate::ui) fn instruct_overseer(&mut self, instruction: &str) {
        let cwd = self
            .ephemeral_root
            .clone()
            .unwrap_or_else(|| self.config.repos_root.clone());
        let result = overseer::ensure_control_session(&self.config, &cwd).and_then(|session| {
            tmux::send_literal_text(&session, instruction)?;
            tmux::send_keys(&session, &["Enter"])
        });
        match result {
            Ok(()) => self.show_message("instruction sent to overseer control"),
            Err(err) => self.show_message(err.to_string()),
        }
    }

    /// Attach the selected Discord channel's tmux session (dropr:371). Unlike
    /// the control AI row there is nothing to create here: a channel session
    /// exists only while a turn is running and is torn down at the end of
    /// it, so an absent session means "no turn is running right now" rather
    /// than "not created yet" — and is said explicitly instead of silently
    /// doing nothing.
    pub(in crate::ui) fn attach_discord_channel_selected(&mut self, index: usize) {
        if !matches!(self.selected_item(), Some(Selection::DiscordChannel(_))) {
            return;
        }
        let ids =
            crate::ui::overseer::ordered_channel_ids(&self.overseer_snapshot.discord_channels);
        let Some(channel_id) = ids.get(index) else {
            self.show_message("channel is no longer listed");
            return;
        };
        let session =
            overseer::discord_channel_session_name(&self.config.tmux_session_prefix, channel_id);
        match tmux::has_session(&session) {
            Ok(true) => self.attach_session(&session),
            Ok(false) => {
                self.show_message("no live session — a turn is not running for this channel");
            }
            Err(err) => self.show_message(err.to_string()),
        }
    }

    pub(in crate::ui) fn attach_orphan_selected(&mut self) {
        let Some(Selection::Orphan(orphan)) = self.selected_item() else {
            return;
        };
        let Some(session) = self.orphans.get(orphan).map(|orphan| orphan.name.clone()) else {
            return;
        };
        self.attach_session(&session);
    }

    pub(in crate::ui) fn restart_selected(&mut self) -> Result<()> {
        if let Some(Selection::Repo(_)) = self.selected_item() {
            let message = match self.refresh_dropr_tasks(true) {
                DroprTaskReload::Running => "reloading dropr tasks…",
                DroprTaskReload::Failed => "failed to start dropr task reload",
                DroprTaskReload::NoLinkedWorkspaces => "no dropr-linked repos",
                DroprTaskReload::OverlayPending => "dropr workspaces not loaded yet",
                DroprTaskReload::OverlayUnavailable => "dropr workspace listing unavailable",
                DroprTaskReload::OverlayDisabled => "dropr overlay is disabled",
            };
            self.show_message(message);
            return Ok(());
        }
        if matches!(self.selected_item(), Some(Selection::ChildWorktree { .. })) {
            self.show_message("restart is not available for child worktrees");
            return Ok(());
        }
        if let Some(Selection::Agent {
            repo,
            agent: agent_idx,
        }) = self.selected_item()
        {
            let selected = self.registry.repos[repo].agents[agent_idx].clone();
            if self.is_merging_agent(&self.registry.repos[repo].path, &selected.id) {
                self.show_message("cannot restart an agent while it is merging");
                return Ok(());
            }
            if selected.status == Status::BranchOnly {
                self.show_message(format!("branch remains: {}", selected.branch));
                return Ok(());
            }
            match agent::restart_agent(&selected) {
                Ok(()) => self.show_message(format!("restarted {}", selected.title)),
                Err(err) => self.show_message(err.to_string()),
            }
        }
        Ok(())
    }
}
