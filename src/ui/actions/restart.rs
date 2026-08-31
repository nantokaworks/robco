use crate::{
    Result, agent,
    locale::{fmt, t},
    model::{Selection, Status},
};

use super::{super::App, dropr_tasks::DroprTaskReload};

impl App {
    pub(in crate::ui) fn restart_selected(&mut self) -> Result<()> {
        if let Some(Selection::Repo(_)) = self.selected_item() {
            let message = match self.refresh_dropr_tasks(true) {
                DroprTaskReload::Running => "reloading dropr tasks…",
                DroprTaskReload::Failed => "failed to start dropr task reload",
                DroprTaskReload::NoLinkedWorkspaces => "no dropr-linked repos",
                DroprTaskReload::OverlayPending => "dropr workspaces not loaded yet",
                DroprTaskReload::OverlayUnavailable => "dropr workspace listing unavailable",
                DroprTaskReload::OverlayDisabled => "dropr overlay is disabled",
                DroprTaskReload::NoMaterialisedWorkspaces => {
                    "linked repos have no materialised dropr board yet"
                }
            };
            self.show_message(t(self.locale, message));
            return Ok(());
        }
        if matches!(self.selected_item(), Some(Selection::ChildWorktree { .. })) {
            self.show_message(t(
                self.locale,
                "restart is not available for child worktrees",
            ));
            return Ok(());
        }
        if let Some(Selection::DiscordChannel(index)) = self.selected_item() {
            self.reset_discord_channel_selected(index);
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
        if self.is_merging_agent(&self.registry.repos[repo].path, &selected.id) {
            self.show_message(t(
                self.locale,
                "cannot restart an agent while it is merging",
            ));
            return Ok(());
        }
        if selected.status == Status::BranchOnly {
            self.show_message(fmt(self.locale, "branch remains: {}", &[&selected.branch]));
            return Ok(());
        }
        if self.registry.repos[repo].host.is_some() {
            let Some(client) = self.remote_client_for_repo(repo) else {
                self.show_message(t(self.locale, "remote host is not connected"));
                return Ok(());
            };
            match client.restart_agent(&selected.id) {
                Ok(outcome) if outcome.ok => {
                    self.show_message(fmt(self.locale, "restarted {}", &[&selected.title]))
                }
                Ok(_) => self.show_message(t(self.locale, "remote restart was refused")),
                Err(error) => self.show_message(error.to_string()),
            }
            return Ok(());
        }
        match agent::restart_agent(&selected) {
            Ok(()) => self.show_message(fmt(self.locale, "restarted {}", &[&selected.title])),
            Err(error) => self.show_message(error.to_string()),
        }
        Ok(())
    }
}
