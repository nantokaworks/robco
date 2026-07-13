use crate::{
    Result, agent, git,
    model::{Selection, Status},
};

use super::super::{App, Mode};

impl App {
    pub(in crate::ui) fn confirm_kill_selected(&mut self) {
        match self.selected_item() {
            Some(Selection::ChildWorktree { .. }) => {
                self.show_message("kill is not available for child worktrees");
            }
            Some(Selection::Agent { repo, agent }) => {
                if self.registry.repos[repo].agents[agent].status == Status::BranchOnly {
                    self.mode = Mode::ConfirmDeleteBranch { repo, agent };
                } else {
                    self.mode = Mode::ConfirmKill { repo, agent };
                }
            }
            Some(Selection::Orphan(orphan)) => {
                if let Some(orphan) = self.orphans.get(orphan) {
                    self.mode = Mode::ConfirmKillOrphan {
                        session: orphan.name.clone(),
                    };
                }
            }
            _ => {}
        }
    }

    pub(in crate::ui) fn kill_agent(&mut self, repo: usize, agent_idx: usize) -> Result<()> {
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
                        self.show_message(format!("killed {}", selected_agent.title));
                    }
                }
                Err(err) => self.show_message(err.to_string()),
            }
        }
        Ok(())
    }

    pub(in crate::ui) fn delete_agent_branch(
        &mut self,
        repo: usize,
        agent_idx: usize,
    ) -> Result<()> {
        if repo < self.registry.repos.len() && agent_idx < self.registry.repos[repo].agents.len() {
            let selected_repo = self.registry.repos[repo].clone();
            let selected_agent = selected_repo.agents[agent_idx].clone();
            match crate::git::delete_branch(&selected_repo.path, &selected_agent.branch) {
                Ok(()) => {
                    self.registry.repos[repo].agents.remove(agent_idx);
                    self.registry.save()?;
                    self.show_message(format!("deleted branch {}", selected_agent.branch));
                }
                Err(err) => self.show_message(err.to_string()),
            }
        }
        Ok(())
    }

    pub(in crate::ui) fn merge_selected(&mut self) {
        if matches!(self.selected_item(), Some(Selection::ChildWorktree { .. })) {
            self.show_message("merge is not available for child worktrees");
            return;
        }
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
            self.show_message(format!("branch remains: {}", selected.branch));
            return;
        }

        match git::worktree_is_clean(&selected.worktree_path) {
            Ok(true) => {}
            Ok(false) => {
                self.show_message("commit or clean untracked changes before merge");
                return;
            }
            Err(err) => {
                self.show_message(err.to_string());
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
                self.show_message(format!(
                    "no open PR for {}; create a PR first",
                    selected.branch
                ));
            }
            Err(err) => self.show_message(err.to_string()),
        }
    }

    pub(in crate::ui) fn perform_merge(&mut self, repo: usize, agent_idx: usize) -> Result<()> {
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
            self.show_message(err.to_string());
            return Ok(());
        }
        if let Err(err) = git::pull_ff_only(&repo_node.path) {
            self.show_message(err.to_string());
            return Ok(());
        }
        if selected.worktree_path.exists()
            && let Err(err) = git::remove_worktree(&repo_node.path, &selected.worktree_path)
        {
            self.show_message(err.to_string());
            return Ok(());
        }
        if git::branch_exists(&repo_node.path, &selected.branch).unwrap_or(false)
            && let Err(err) = git::delete_branch(&repo_node.path, &selected.branch)
        {
            self.show_message(err.to_string());
            return Ok(());
        }
        let _ = git::delete_remote_branch(&repo_node.path, &selected.branch);

        self.registry.repos[repo].agents.remove(agent_idx);
        self.registry.save()?;
        self.show_message(format!("merged & landed {}", selected.branch));
        Ok(())
    }

    pub(in crate::ui) fn add_repo_path(&mut self, path: &str) {
        let path = std::path::PathBuf::from(path);
        if !crate::discover::is_git_repo(&path) {
            self.show_message("path is not a git repository");
            return;
        }

        let Ok(path) = path.canonicalize() else {
            self.show_message("could not resolve path");
            return;
        };

        if self.registry.repos.iter().any(|repo| repo.path == path) {
            self.show_message("repository already listed");
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
            main_status: None,
            main_last_capture: None,
            main_last_change_at: None,
            main_shell_working: false,
            main_pane_pid: None,
            main_tracked_command: None,
            main_subagents_active: 0,
        });
        self.expanded.push(true);
        if let Err(err) = self.registry.save() {
            self.show_message(err.to_string());
        } else {
            self.show_message("repository added");
        }
    }
}
