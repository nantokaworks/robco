use crate::{
    Result, git,
    locale::{fmt, t},
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
            self.show_message(t(self.locale, "repository changed, not removed"));
            return Ok(());
        };
        if !self.registry.repos[repo].pinned || !self.registry.repos[repo].agents.is_empty() {
            self.show_message(t(self.locale, "repository changed, not removed"));
            return Ok(());
        }

        let removed = self.registry.repos[repo].name.clone();
        let path = path.to_path_buf();
        // Re-check the precondition against the stored row instead of dropping
        // the repo on the strength of this process's snapshot: another writer
        // may have registered an agent under it since the registry was read.
        self.locked_registry_update(|registry| {
            registry.repos.retain(|stored| {
                stored.path != path || !(stored.pinned && stored.agents.is_empty())
            })
        })?;
        self.clamp_selection();
        self.show_message(fmt(self.locale, "removed {}", &[&removed]));
        Ok(())
    }

    pub(in crate::ui) fn merge_selected(&mut self) {
        if matches!(self.selected_item(), Some(Selection::ChildWorktree { .. })) {
            self.show_message(t(self.locale, "merge is not available for child worktrees"));
            return;
        }
        let Some(Selection::Agent {
            repo,
            agent: agent_idx,
        }) = self.selected_item()
        else {
            return;
        };

        // Only this repository's in-flight merge blocks another one; merges in
        // other repositories touch none of the state `run_merge` races on.
        let repo_path = self.registry.repos[repo].path.clone();
        let repo_name = self.registry.repos[repo].name.clone();
        if let Some(running) = self.merge_job(&repo_path).map(|job| job.branch.clone()) {
            self.show_message(fmt(
                self.locale,
                "merge already in progress in {}: {}",
                &[&repo_name, &running],
            ));
            return;
        }

        self.registry.repos[repo].agents[agent_idx].merge_error = None;
        let repo_node = self.registry.repos[repo].clone();
        let selected = repo_node.agents[agent_idx].clone();
        if selected.status == Status::BranchOnly {
            self.show_message(fmt(self.locale, "branch remains: {}", &[&selected.branch]));
            return;
        }

        match git::worktree_is_clean(&selected.worktree_path) {
            Ok(true) => {}
            Ok(false) => {
                self.show_message(t(
                    self.locale,
                    "commit or clean untracked changes before merge",
                ));
                return;
            }
            Err(err) => {
                self.show_message(err.to_string());
                return;
            }
        }

        match git::pr_state(&repo_node.path, &selected.branch) {
            Ok(state) => self.offer_merge_or_cleanup(state, repo, agent_idx, &selected.branch),
            Err(err) => self.show_message(err.to_string()),
        }
    }

    /// Routes `m` by what the branch's pull request actually is. A merge that
    /// landed elsewhere — the Overseer's auto-merge pass, the GitHub web UI,
    /// another terminal — leaves the merge step done and the cleanup after it
    /// undone, which is the only reason this agent is still in the tree.
    pub(in crate::ui) fn offer_merge_or_cleanup(
        &mut self,
        state: git::PrState,
        repo: usize,
        agent_idx: usize,
        branch: &str,
    ) {
        match state {
            git::PrState::Open => {
                self.mode = Mode::ConfirmMerge {
                    repo,
                    agent: agent_idx,
                }
            }
            git::PrState::Merged => {
                self.mode = Mode::ConfirmCleanup {
                    repo,
                    agent: agent_idx,
                }
            }
            // Closed without merging: the branch still holds the only copy of
            // its work, so removing the worktree and deleting the branch would
            // destroy it.
            git::PrState::ClosedUnmerged => self.show_message(fmt(
                self.locale,
                "PR for {} was closed without merging; reopen it or open a new one",
                &[branch],
            )),
            git::PrState::Absent => self.show_message(fmt(
                self.locale,
                "no open PR for {}; create a PR first",
                &[branch],
            )),
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
            self.show_message(t(self.locale, "path is not a git repository"));
            return;
        }

        let Ok(path) = path.canonicalize() else {
            self.show_message(t(self.locale, "could not resolve path"));
            return;
        };

        let mut changed = false;
        if let Err(err) =
            self.locked_registry_update(|registry| changed = registry.add_canonical_pinned(path))
        {
            self.show_message(err.to_string());
        } else if !changed {
            self.show_message(t(self.locale, "repository already listed"));
        } else {
            self.show_message(t(self.locale, "repository added"));
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
