use crate::{
    Result, agent, git,
    model::{Selection, Status},
    tmux,
};

use super::{App, Mode, suspend_terminal};

impl App {
    pub(super) fn attach_selected(&mut self) -> Result<()> {
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

    pub(super) fn attach_shell_selected(&mut self) -> Result<()> {
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
        match agent::ensure_shell_session(&selected) {
            Ok(()) => {
                let session = agent::shell_session_name(&selected);
                self.force_redraw = true;
                suspend_terminal(|| tmux::attach(&session))?;
            }
            Err(err) => self.mode = Mode::Message(err.to_string()),
        }
        Ok(())
    }

    pub(super) fn restart_selected(&mut self) -> Result<()> {
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

    pub(super) fn confirm_kill_selected(&mut self) {
        if let Some(Selection::Agent { repo, agent }) = self.selected_item() {
            if self.registry.repos[repo].agents[agent].status == Status::BranchOnly {
                self.mode = Mode::ConfirmDeleteBranch { repo, agent };
            } else {
                self.mode = Mode::ConfirmKill { repo, agent };
            }
        }
    }

    pub(super) fn kill_agent(&mut self, repo: usize, agent_idx: usize) -> Result<()> {
        if repo < self.registry.repos.len() && agent_idx < self.registry.repos[repo].agents.len() {
            let selected_repo = self.registry.repos[repo].clone();
            let selected_agent = selected_repo.agents[agent_idx].clone();
            match agent::kill_agent(&selected_repo, &selected_agent) {
                Ok(()) => {
                    if crate::git::branch_exists(&selected_repo.path, &selected_agent.branch)
                        .unwrap_or(false)
                    {
                        self.registry.repos[repo].agents[agent_idx].status = Status::BranchOnly;
                        self.registry.save()?;
                        self.mode = Mode::ConfirmDeleteBranch {
                            repo,
                            agent: agent_idx,
                        };
                    } else {
                        self.registry.repos[repo].agents.remove(agent_idx);
                        self.registry.save()?;
                        self.mode = Mode::Message(format!("killed {}", selected_agent.title));
                    }
                }
                Err(err) => self.mode = Mode::Message(err.to_string()),
            }
        }
        Ok(())
    }

    pub(super) fn delete_agent_branch(&mut self, repo: usize, agent_idx: usize) -> Result<()> {
        if repo < self.registry.repos.len() && agent_idx < self.registry.repos[repo].agents.len() {
            let selected_repo = self.registry.repos[repo].clone();
            let selected_agent = selected_repo.agents[agent_idx].clone();
            match crate::git::delete_branch(&selected_repo.path, &selected_agent.branch) {
                Ok(()) => {
                    self.registry.repos[repo].agents.remove(agent_idx);
                    self.registry.save()?;
                    self.mode = Mode::Message(format!("deleted branch {}", selected_agent.branch));
                }
                Err(err) => self.mode = Mode::Message(err.to_string()),
            }
        }
        Ok(())
    }

    pub(super) fn ship_selected(&mut self) {
        if let Some(Selection::Agent {
            repo,
            agent: agent_idx,
        }) = self.selected_item()
        {
            let selected = self.registry.repos[repo].agents[agent_idx].clone();
            if selected.status == Status::BranchOnly {
                self.mode = Mode::Message(format!("branch remains: {}", selected.branch));
                return;
            }
            match agent::ship_agent(&selected) {
                Ok(()) => self.mode = Mode::Message(format!("pushed {}", selected.branch)),
                Err(err) => self.mode = Mode::Message(err.to_string()),
            }
        }
    }

    pub(super) fn merge_selected(&mut self) {
        let Some(Selection::Agent {
            repo,
            agent: agent_idx,
        }) = self.selected_item()
        else {
            return;
        };

        let repo_node = self.registry.repos[repo].clone();
        let selected = repo_node.agents[agent_idx].clone();
        if selected.status == Status::BranchOnly {
            self.mode = Mode::Message(format!("branch remains: {}", selected.branch));
            return;
        }

        match git::tracked_tree_is_clean(&selected.worktree_path) {
            Ok(true) => {}
            Ok(false) => {
                self.mode = Mode::Message(
                    "commit changes before merge (press s to ship, or commit first)".to_string(),
                );
                return;
            }
            Err(err) => {
                self.mode = Mode::Message(err.to_string());
                return;
            }
        }

        match git::pr_exists(&repo_node.path, &selected.branch) {
            Ok(true) => {
                self.mode = Mode::ConfirmMerge {
                    repo,
                    agent: agent_idx,
                }
            }
            Ok(false) => {
                self.mode = Mode::Message(format!(
                    "no open PR for {}; create a PR first",
                    selected.branch
                ));
            }
            Err(err) => self.mode = Mode::Message(err.to_string()),
        }
    }

    pub(super) fn perform_merge(&mut self, repo: usize, agent_idx: usize) -> Result<()> {
        if repo >= self.registry.repos.len() || agent_idx >= self.registry.repos[repo].agents.len()
        {
            return Ok(());
        }

        let repo_node = self.registry.repos[repo].clone();
        let selected = repo_node.agents[agent_idx].clone();
        if let Err(err) = git::merge_pr(
            &repo_node.path,
            &selected.branch,
            self.config.merge_strategy.gh_flag(),
        ) {
            self.mode = Mode::Message(err.to_string());
            return Ok(());
        }
        if let Err(err) = git::pull_ff_only(&repo_node.path) {
            self.mode = Mode::Message(err.to_string());
            return Ok(());
        }
        if selected.worktree_path.exists()
            && let Err(err) = git::remove_worktree(&repo_node.path, &selected.worktree_path)
        {
            self.mode = Mode::Message(err.to_string());
            return Ok(());
        }
        if git::branch_exists(&repo_node.path, &selected.branch).unwrap_or(false)
            && let Err(err) = git::delete_branch(&repo_node.path, &selected.branch)
        {
            self.mode = Mode::Message(err.to_string());
            return Ok(());
        }
        let _ = git::delete_remote_branch(&repo_node.path, &selected.branch);

        self.registry.repos[repo].agents.remove(agent_idx);
        self.registry.save()?;
        self.mode = Mode::Message(format!("merged & landed {}", selected.branch));
        Ok(())
    }

    pub(super) fn add_repo_path(&mut self, path: &str) {
        let path = std::path::PathBuf::from(path);
        if !crate::discover::is_git_repo(&path) {
            self.mode = Mode::Message("path is not a git repository".to_string());
            return;
        }

        let Ok(path) = path.canonicalize() else {
            self.mode = Mode::Message("could not resolve path".to_string());
            return;
        };

        if self.registry.repos.iter().any(|repo| repo.path == path) {
            self.mode = Mode::Message("repository already listed".to_string());
            return;
        }

        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("repo")
            .to_string();
        let remote_url = crate::git::remote_url(&path).ok();
        self.registry.repos.push(crate::model::RepoNode {
            path,
            name,
            remote_url,
            agents: Vec::new(),
            dropr: None,
        });
        self.expanded.push(true);
        if let Err(err) = self.registry.save() {
            self.mode = Mode::Message(err.to_string());
        } else {
            self.mode = Mode::Message("repository added".to_string());
        }
    }
}
