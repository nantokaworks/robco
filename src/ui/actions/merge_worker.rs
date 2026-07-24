//! The off-thread half of a merge: the `git` / `gh` sequence run for one
//! repository, and the events it reports back to the UI thread.
//!
//! The sequence races on the target repository's `main` branch and working
//! tree, which is why [`super::merge`] serialises merges per repository. It
//! touches nothing outside that repository, which is why merges in different
//! repositories run concurrently.

use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use crate::{Result, git};

pub(super) const MERGING_PR: &str = "merging PR";
pub(super) const PULLING_MAIN: &str = "pulling main";
pub(super) const CLEANING_UP: &str = "cleaning up";
pub(super) const WORKER_TERMINATED: &str = "merge worker terminated unexpectedly";

#[derive(Debug)]
pub(super) enum MergeEvent {
    Step(&'static str),
    Finished(std::result::Result<(), String>),
}

pub(super) struct MergeTarget {
    pub repo_path: PathBuf,
    pub branch: String,
    pub strategy: &'static str,
    pub worktree_path: PathBuf,
    pub tmux_session: String,
    pub shell_session: String,
}

pub(super) fn spawn(target: MergeTarget) -> Receiver<MergeEvent> {
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
