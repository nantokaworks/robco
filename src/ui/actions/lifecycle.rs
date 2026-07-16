use crate::{
    Result, git,
    model::{RepoNode, Selection, Status},
};
use std::path::Path;

use super::super::{App, Mode};

pub(super) fn resolve_agent(
    repos: &[RepoNode],
    repo_path: &Path,
    agent_id: &str,
) -> Option<(usize, usize)> {
    let repo = repos.iter().position(|repo| repo.path == repo_path)?;
    let agent = repos[repo]
        .agents
        .iter()
        .position(|agent| agent.id == agent_id)?;
    Some((repo, agent))
}

fn set_merge_error(error: &mut Option<String>, detail: &str) {
    *error = Some(detail.to_string());
}

impl App {
    pub(in crate::ui) fn remove_pinned_repo(&mut self, path: &Path) -> Result<()> {
        let Some(repo) = self
            .registry
            .repos
            .iter()
            .position(|repo| repo.path == path)
        else {
            self.show_message("repository changed, not removed");
            return Ok(());
        };
        if !self.registry.repos[repo].pinned || !self.registry.repos[repo].agents.is_empty() {
            self.show_message("repository changed, not removed");
            return Ok(());
        }

        let removed = self.registry.repos.remove(repo);
        if repo < self.expanded.len() {
            self.expanded.remove(repo);
        }
        self.registry.save()?;
        self.clamp_selection();
        self.show_message(format!("removed {}", removed.name));
        Ok(())
    }

    pub(in crate::ui) fn merge_selected(&mut self) {
        if let Some(job) = self.merge_job() {
            self.show_message(format!("merge already in progress: {}", job.branch));
            return;
        }
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

        self.registry.repos[repo].agents[agent_idx].merge_error = None;
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

    pub(in crate::ui) fn record_merge_error(
        &mut self,
        repo: usize,
        agent_idx: usize,
        detail: String,
    ) {
        set_merge_error(
            &mut self.registry.repos[repo].agents[agent_idx].merge_error,
            &detail,
        );
        self.show_message(detail);
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

        let previous_len = self.registry.repos.len();
        let changed = match self.registry.add_pinned(&path) {
            Ok(added) => added,
            Err(err) => {
                self.show_message(err.to_string());
                return;
            }
        };
        if self.registry.repos.len() > previous_len {
            self.expanded.push(true);
        }
        if let Err(err) = self.registry.save() {
            self.show_message(err.to_string());
        } else if !changed {
            self.show_message("repository already listed");
        } else {
            self.show_message("repository added");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn merge_error_is_recorded_and_cleared_for_the_next_attempt() {
        let mut error = None;

        set_merge_error(&mut error, "gh failed\nretry later");
        assert_eq!(error.as_deref(), Some("gh failed\nretry later"));

        error = None;
        assert_eq!(error, None);
    }
}
