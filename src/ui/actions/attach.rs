use crate::{
    Result, agent,
    model::{Selection, Status},
    tmux,
};

use super::super::{App, Mode, suspend_terminal};

impl App {
    /// Suspend the TUI and hand the terminal to a tmux session. A failure here
    /// (e.g. the session exited between `ensure_*` and attach, or a transient
    /// tmux error) is surfaced as an in-app message and MUST NOT propagate: a
    /// bubbling `Err` would unwind out of the event loop and exit robco, which
    /// over ssh drops the whole connection.
    fn attach_session(&mut self, session: &str) {
        self.force_redraw = true;
        if let Err(err) = suspend_terminal(|| tmux::attach(session)) {
            self.mode = Mode::Message(err.to_string());
        }
    }

    pub(in crate::ui) fn attach_selected(&mut self) -> Result<()> {
        let Some(Selection::Agent {
            repo,
            agent: agent_idx,
        }) = self.selected_item()
        else {
            return Ok(());
        };
        let selected = self.registry.repos[repo].agents[agent_idx].clone();
        if selected.status == Status::BranchOnly {
            self.mode = Mode::Message(format!("branch remains: {}", selected.branch));
            return Ok(());
        }
        match agent::ensure_agent_session(&selected) {
            Ok(()) => self.attach_session(&selected.tmux_session),
            Err(err) => self.mode = Mode::Message(err.to_string()),
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
                    self.mode = Mode::Message(format!("branch remains: {}", selected.branch));
                    return Ok(());
                }
                match agent::ensure_shell_session(&selected) {
                    Ok(()) => self.attach_session(&agent::shell_session_name(&selected)),
                    Err(err) => self.mode = Mode::Message(err.to_string()),
                }
            }
            Some(Selection::Repo(repo)) => {
                let repo_node = self.registry.repos[repo].clone();
                let prefix = self.config.tmux_session_prefix.clone();
                match agent::ensure_repo_shell_session(&prefix, &repo_node) {
                    Ok(()) => {
                        self.attach_session(&agent::repo_shell_session_name(&prefix, &repo_node))
                    }
                    Err(err) => self.mode = Mode::Message(err.to_string()),
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
                    self.mode = Mode::Message(format!("branch remains: {}", selected.branch));
                    return Ok(());
                }
                match agent::ensure_agent_session(&selected) {
                    Ok(()) => self.attach_session(&selected.tmux_session),
                    Err(err) => self.mode = Mode::Message(err.to_string()),
                }
            }
            Some(Selection::Repo(repo)) => {
                let repo_node = self.registry.repos[repo].clone();
                let prefix = self.config.tmux_session_prefix.clone();
                match agent::ensure_repo_claude_session(&self.config, &prefix, &repo_node) {
                    Ok(()) => {
                        self.attach_session(&agent::repo_claude_session_name(&prefix, &repo_node))
                    }
                    Err(err) => self.mode = Mode::Message(err.to_string()),
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
        if let Some(Selection::Agent {
            repo,
            agent: agent_idx,
        }) = self.selected_item()
        {
            let selected = self.registry.repos[repo].agents[agent_idx].clone();
            if selected.status == Status::BranchOnly {
                self.mode = Mode::Message(format!("branch remains: {}", selected.branch));
                return Ok(());
            }
            match agent::restart_agent(&selected) {
                Ok(()) => self.mode = Mode::Message(format!("restarted {}", selected.title)),
                Err(err) => self.mode = Mode::Message(err.to_string()),
            }
        }
        Ok(())
    }
}
