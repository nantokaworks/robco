use crate::{
    Result, agent,
    model::{Selection, Status},
    tmux,
};

use super::super::{App, Mode, suspend_terminal};

impl App {
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
            Ok(()) => {
                let session = selected.tmux_session.clone();
                self.force_redraw = true;
                suspend_terminal(|| tmux::attach(&session))?;
            }
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
                    Ok(()) => {
                        let session = agent::shell_session_name(&selected);
                        self.force_redraw = true;
                        suspend_terminal(|| tmux::attach(&session))?;
                    }
                    Err(err) => self.mode = Mode::Message(err.to_string()),
                }
            }
            Some(Selection::Repo(repo)) => {
                let repo_node = self.registry.repos[repo].clone();
                let prefix = self.config.tmux_session_prefix.clone();
                match agent::ensure_repo_shell_session(&prefix, &repo_node) {
                    Ok(()) => {
                        let session = agent::repo_shell_session_name(&prefix, &repo_node);
                        self.force_redraw = true;
                        suspend_terminal(|| tmux::attach(&session))?;
                    }
                    Err(err) => self.mode = Mode::Message(err.to_string()),
                }
            }
            None => {}
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
                    Ok(()) => {
                        let session = selected.tmux_session.clone();
                        self.force_redraw = true;
                        suspend_terminal(|| tmux::attach(&session))?;
                    }
                    Err(err) => self.mode = Mode::Message(err.to_string()),
                }
            }
            Some(Selection::Repo(repo)) => {
                let repo_node = self.registry.repos[repo].clone();
                let prefix = self.config.tmux_session_prefix.clone();
                match agent::ensure_repo_claude_session(&self.config, &prefix, &repo_node) {
                    Ok(()) => {
                        let session = agent::repo_claude_session_name(&prefix, &repo_node);
                        self.force_redraw = true;
                        suspend_terminal(|| tmux::attach(&session))?;
                    }
                    Err(err) => self.mode = Mode::Message(err.to_string()),
                }
            }
            None => {}
        }
        Ok(())
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
