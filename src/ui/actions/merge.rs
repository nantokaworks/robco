use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread,
};

use crate::{Result, agent, git};

use super::{
    super::{App, Mode},
    lifecycle::resolve_agent,
};

const MERGING_PR: &str = "merging PR";
const PULLING_MAIN: &str = "pulling main";
const CLEANING_UP: &str = "cleaning up";
const WORKER_TERMINATED: &str = "merge worker terminated unexpectedly";

#[derive(Debug)]
pub(in crate::ui) enum MergeEvent {
    Step(&'static str),
    Finished(std::result::Result<(), String>),
}

pub(in crate::ui) struct MergeJob {
    pub repo_path: PathBuf,
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

struct MergeTarget {
    repo_path: PathBuf,
    branch: String,
    strategy: &'static str,
    worktree_path: PathBuf,
    tmux_session: String,
    shell_session: String,
}

impl App {
    /// Clear a lingering merge outcome banner. No-op (returns false) while a
    /// merge job is still running, so an in-progress merge cannot be dismissed.
    pub(in crate::ui) fn dismiss_merge_outcome(&mut self) -> bool {
        if self.merge_job.is_some() {
            return false;
        }
        self.merge_outcome.take().is_some()
    }

    pub(in crate::ui) fn start_merge(&mut self, repo: usize, agent_idx: usize) {
        if let Some(job) = &self.merge_job {
            self.mode = super::super::Mode::Normal;
            self.show_message(format!("merge already in progress: {}", job.branch));
            return;
        }
        let Some(repo_node) = self.registry.repos.get(repo) else {
            self.mode = super::super::Mode::Normal;
            return;
        };
        let Some(selected) = repo_node.agents.get(agent_idx) else {
            self.mode = super::super::Mode::Normal;
            return;
        };
        let repo_path = repo_node.path.clone();
        let agent_id = selected.id.clone();
        let branch = selected.branch.clone();
        let target = MergeTarget {
            repo_path: repo_path.clone(),
            branch: branch.clone(),
            strategy: self.config.merge_strategy.gh_flag(),
            worktree_path: selected.worktree_path.clone(),
            tmux_session: selected.tmux_session.clone(),
            shell_session: agent::shell_session_name(selected),
        };

        self.merge_outcome = None;
        self.merge_job = Some(MergeJob {
            repo_path,
            agent_id,
            branch,
            step: MERGING_PR,
            receiver: spawn(target),
        });
        self.mode = super::super::Mode::Normal;
    }

    pub(in crate::ui) fn drain_merge_events(&mut self) -> Result<()> {
        let mut events = Vec::new();
        let mut disconnected = false;
        if let Some(job) = &self.merge_job {
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
        }
        for event in events {
            match event {
                MergeEvent::Step(next) => {
                    if let Some(job) = &mut self.merge_job {
                        job.step = next;
                    }
                }
                MergeEvent::Finished(result) => self.finish_merge(result)?,
            }
        }
        if disconnected && self.merge_job.is_some() {
            self.finish_merge(Err(WORKER_TERMINATED.into()))?;
        }
        Ok(())
    }

    fn finish_merge(&mut self, result: std::result::Result<(), String>) -> Result<()> {
        self.finish_merge_with(result, crate::registry::Registry::save)
    }

    fn finish_merge_with(
        &mut self,
        result: std::result::Result<(), String>,
        save: impl FnOnce(&crate::registry::Registry) -> Result<()>,
    ) -> Result<()> {
        let Some(job) = self.merge_job.take() else {
            return Ok(());
        };
        let MergeJob {
            repo_path,
            agent_id,
            branch,
            ..
        } = job;
        self.merge_outcome = Some(MergeOutcome {
            repo_path: repo_path.clone(),
            agent_id: agent_id.clone(),
            branch: branch.clone(),
            result: result.clone(),
        });

        match result {
            Ok(()) => {
                if let Some((repo, agent)) =
                    resolve_agent(&self.registry.repos, &repo_path, &agent_id)
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
                    resolve_agent(&self.registry.repos, &repo_path, &agent_id)
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

    pub(in crate::ui) fn merge_job(&self) -> Option<&MergeJob> {
        self.merge_job.as_ref()
    }

    pub(in crate::ui) fn merge_outcome(&self) -> Option<&MergeOutcome> {
        self.merge_outcome.as_ref()
    }

    pub(in crate::ui) fn is_merging_agent(
        &self,
        repo_path: &std::path::Path,
        agent_id: &str,
    ) -> bool {
        self.merge_job
            .as_ref()
            .is_some_and(|job| job.repo_path == repo_path && job.agent_id == agent_id)
    }
}

fn spawn(target: MergeTarget) -> Receiver<MergeEvent> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = run_merge(&target, &sender).map_err(|error| error.to_string());
        let _ = sender.send(MergeEvent::Finished(result));
    });
    receiver
}

fn run_merge(target: &MergeTarget, sender: &Sender<MergeEvent>) -> Result<()> {
    git::merge_pr(&target.repo_path, &target.branch, target.strategy)?;
    send_step(sender, PULLING_MAIN);
    git::pull_ff_only(&target.repo_path)?;
    send_step(sender, CLEANING_UP);
    if target.worktree_path.exists() {
        git::remove_worktree(&target.repo_path, &target.worktree_path, false)?;
    }
    if git::branch_exists(&target.repo_path, &target.branch)? {
        git::delete_branch(&target.repo_path, &target.branch)?;
    }
    let _ = git::delete_remote_branch(&target.repo_path, &target.branch);
    let _ = crate::tmux::kill_session(&target.tmux_session);
    let _ = crate::tmux::kill_session(&target.shell_session);
    Ok(())
}

fn send_step(sender: &Sender<MergeEvent>, step: &'static str) {
    let _ = sender.send(MergeEvent::Step(step));
}

#[cfg(test)]
#[path = "merge_tests.rs"]
mod tests;
