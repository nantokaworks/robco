use crate::{
    Result, agent,
    model::{Selection, Status},
    tmux,
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
            let message = match self.refresh_dropr_tasks() {
                DroprTaskReload::Running => "reloading dropr tasks…",
                DroprTaskReload::Failed => "failed to start dropr task reload",
                DroprTaskReload::NoLinkedWorkspaces => "no dropr-linked repos",
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
