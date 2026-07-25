use std::{
    path::{Path, PathBuf},
    sync::mpsc::{Receiver, TryRecvError},
};

use crate::{Result, agent};

use super::{
    super::{App, Mode},
    lifecycle::resolve_agent,
    merge_worker::{MergeEvent, MergeMode, MergeTarget, WORKER_TERMINATED, spawn},
};

/// One in-flight merge. Jobs live in [`App::merge_jobs`] keyed by repository
/// path, which is why the repository is not repeated here.
pub(in crate::ui) struct MergeJob {
    pub agent_id: String,
    pub branch: String,
    pub step: &'static str,
    receiver: Receiver<MergeEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui) struct MergeOutcome {
    pub repo_path: PathBuf,
    pub agent_id: String,
    pub branch: String,
    pub result: std::result::Result<(), String>,
}

impl App {
    /// Clear the lingering merge outcome banner of the selected repository.
    /// Outcomes are per repository and each banner only renders while its own
    /// agent is selected, so dismissal is scoped the same way — an `esc` aimed
    /// at one repository never discards another repository's result. No-op
    /// (returns false) while the selected repository's merge is still running,
    /// so an in-progress merge cannot be dismissed.
    pub(in crate::ui) fn dismiss_merge_outcome(&mut self) -> bool {
        let Some(repo_path) = self
            .selected_repo()
            .and_then(|repo| self.registry.repos.get(repo))
            .map(|repo_node| repo_node.path.clone())
        else {
            return false;
        };
        if self.merge_jobs.contains_key(&repo_path) {
            return false;
        }
        self.merge_outcomes.remove(&repo_path).is_some()
    }

    pub(in crate::ui) fn start_merge(&mut self, repo: usize, agent_idx: usize) {
        self.start_merge_job(repo, agent_idx, MergeMode::MergeThenClean);
    }

    /// Runs the post-merge sequence alone, for an agent whose pull request
    /// merged somewhere other than this key.
    pub(in crate::ui) fn start_cleanup(&mut self, repo: usize, agent_idx: usize) {
        self.start_merge_job(repo, agent_idx, MergeMode::CleanOnly);
    }

    fn start_merge_job(&mut self, repo: usize, agent_idx: usize, mode: MergeMode) {
        self.mode = Mode::Normal;
        let Some(repo_node) = self.registry.repos.get(repo) else {
            return;
        };
        let Some(selected) = repo_node.agents.get(agent_idx) else {
            return;
        };
        let repo_path = repo_node.path.clone();
        let repo_name = repo_node.name.clone();
        let agent_id = selected.id.clone();
        let branch = selected.branch.clone();
        let target = MergeTarget {
            repo_path: repo_path.clone(),
            branch: branch.clone(),
            mode,
            strategy: self.config.merge_strategy.gh_flag(),
            worktree_path: selected.worktree_path.clone(),
            tmux_session: selected.tmux_session.clone(),
            shell_session: agent::shell_session_name(selected),
        };

        // Merges are serialised per repository — `run_merge` races on that
        // repository's main branch and working tree — but repositories share
        // no state, so a merge elsewhere is none of this repository's business.
        if let Some(running) = self
            .merge_jobs
            .get(&repo_path)
            .map(|job| job.branch.clone())
        {
            self.show_message(format!(
                "merge already in progress in {repo_name}: {running}"
            ));
            return;
        }

        self.merge_outcomes.remove(&repo_path);
        self.merge_jobs.insert(
            repo_path,
            MergeJob {
                agent_id,
                branch,
                step: mode.first_step(),
                receiver: spawn(target),
            },
        );
    }

    pub(in crate::ui) fn drain_merge_events(&mut self) -> Result<()> {
        let mut drained = Vec::new();
        for (repo_path, job) in &self.merge_jobs {
            let mut events = Vec::new();
            let mut disconnected = false;
            loop {
                match job.receiver.try_recv() {
                    Ok(event) => events.push(event),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
            drained.push((repo_path.clone(), events, disconnected));
        }
        for (repo_path, events, disconnected) in drained {
            for event in events {
                match event {
                    MergeEvent::Step(next) => {
                        if let Some(job) = self.merge_jobs.get_mut(&repo_path) {
                            job.step = next;
                        }
                    }
                    MergeEvent::Finished(result) => self.finish_merge(&repo_path, result)?,
                }
            }
            if disconnected && self.merge_jobs.contains_key(&repo_path) {
                self.finish_merge(&repo_path, Err(WORKER_TERMINATED.into()))?;
            }
        }
        Ok(())
    }

    fn finish_merge(
        &mut self,
        repo_path: &Path,
        result: std::result::Result<(), String>,
    ) -> Result<()> {
        self.finish_merge_with(repo_path, result, crate::registry::Registry::save)
    }

    fn finish_merge_with(
        &mut self,
        repo_path: &Path,
        result: std::result::Result<(), String>,
        save: impl FnOnce(&crate::registry::Registry) -> Result<()>,
    ) -> Result<()> {
        let Some(job) = self.merge_jobs.remove(repo_path) else {
            return Ok(());
        };
        let MergeJob {
            agent_id, branch, ..
        } = job;
        self.merge_outcomes.insert(
            repo_path.to_path_buf(),
            MergeOutcome {
                repo_path: repo_path.to_path_buf(),
                agent_id: agent_id.clone(),
                branch: branch.clone(),
                result: result.clone(),
            },
        );

        match result {
            Ok(()) => {
                if let Some((repo, agent)) =
                    resolve_agent(&self.registry.repos, repo_path, &agent_id)
                {
                    self.registry.repos[repo].agents.remove(agent);
                    let dialog_closed = self.remap_dialog_after_agent_removal(repo, agent);
                    save(&self.registry)?;
                    if dialog_closed {
                        self.show_message("closed dialog because its agent was merged");
                    } else {
                        self.show_message(format!("merge complete: {branch}"));
                    }
                } else {
                    self.show_message(format!("merge complete: {branch}"));
                }
                self.clamp_selection();
            }
            Err(detail) => {
                if let Some((repo, agent)) =
                    resolve_agent(&self.registry.repos, repo_path, &agent_id)
                {
                    self.record_merge_error(repo, agent, detail);
                } else {
                    self.show_message(detail);
                }
            }
        }
        Ok(())
    }

    fn remap_dialog_after_agent_removal(&mut self, repo: usize, removed: usize) -> bool {
        let agent = match &mut self.mode {
            Mode::ConfirmKill {
                repo: dialog_repo,
                agent,
            }
            | Mode::ConfirmMerge {
                repo: dialog_repo,
                agent,
            }
            | Mode::ConfirmCleanup {
                repo: dialog_repo,
                agent,
            }
            | Mode::ConfirmDeleteBranch {
                repo: dialog_repo,
                agent,
            } if *dialog_repo == repo => agent,
            _ => return false,
        };

        match (*agent).cmp(&removed) {
            std::cmp::Ordering::Greater => {
                *agent -= 1;
                false
            }
            std::cmp::Ordering::Equal => {
                self.mode = Mode::Normal;
                true
            }
            std::cmp::Ordering::Less => false,
        }
    }

    pub(in crate::ui) fn merge_job(&self, repo_path: &Path) -> Option<&MergeJob> {
        self.merge_jobs.get(repo_path)
    }

    pub(in crate::ui) fn merge_outcome(&self, repo_path: &Path) -> Option<&MergeOutcome> {
        self.merge_outcomes.get(repo_path)
    }

    /// Branches of every merge currently in flight, sorted so the quit warning
    /// reads the same on every frame — `HashMap` iteration order does not.
    pub(in crate::ui) fn merging_branches(&self) -> Vec<String> {
        let mut branches: Vec<String> = self
            .merge_jobs
            .values()
            .map(|job| job.branch.clone())
            .collect();
        branches.sort();
        branches
    }

    pub(in crate::ui) fn is_merging_agent(&self, repo_path: &Path, agent_id: &str) -> bool {
        self.merge_jobs
            .get(repo_path)
            .is_some_and(|job| job.agent_id == agent_id)
    }
}

#[cfg(test)]
#[path = "merge_tests.rs"]
mod tests;
