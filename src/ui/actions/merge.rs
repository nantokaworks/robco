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

struct MergeTarget {
    repo_path: PathBuf,
    branch: String,
    strategy: &'static str,
    worktree_path: PathBuf,
    tmux_session: String,
    shell_session: String,
}

impl App {
    pub(in crate::ui) fn start_merge(&mut self, repo: usize, agent_idx: usize) {
        let Some(repo_node) = self.registry.repos.get(repo) else {
            self.mode = Mode::Normal;
            return;
        };
        let Some(selected) = repo_node.agents.get(agent_idx) else {
            self.mode = Mode::Normal;
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

        self.merge_receiver = Some(spawn(target));
        self.mode = Mode::MergeInProgress {
            repo_path,
            agent_id,
            branch,
            step: MERGING_PR,
        };
    }

    pub(in crate::ui) fn drain_merge_events(&mut self) -> Result<()> {
        let mut events = Vec::new();
        let mut disconnected = false;
        if let Some(receiver) = &self.merge_receiver {
            loop {
                match receiver.try_recv() {
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
                    if let Mode::MergeInProgress { step, .. } = &mut self.mode {
                        *step = next;
                    }
                }
                MergeEvent::Finished(result) => self.finish_merge(result)?,
            }
        }
        if disconnected && self.merge_receiver.is_some() {
            self.finish_merge(Err(WORKER_TERMINATED.into()))?;
        }
        Ok(())
    }

    fn finish_merge(&mut self, result: std::result::Result<(), String>) -> Result<()> {
        self.merge_receiver = None;
        let Mode::MergeInProgress {
            repo_path,
            agent_id,
            branch,
            ..
        } = &self.mode
        else {
            return Ok(());
        };
        let repo_path = repo_path.clone();
        let agent_id = agent_id.clone();
        let branch = branch.clone();

        match result {
            Ok(()) => {
                if let Some((repo, agent)) =
                    resolve_agent(&self.registry.repos, &repo_path, &agent_id)
                {
                    self.registry.repos[repo].agents.remove(agent);
                    self.registry.save()?;
                }
                self.mode = Mode::MergeComplete { branch };
            }
            Err(detail) => {
                self.mode = Mode::Normal;
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
        git::remove_worktree(&target.repo_path, &target.worktree_path)?;
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
