//! The off-thread half of a merge: the worker thread that runs the shared merge
//! sequence for one repository, and the events it reports back to the UI thread.
//!
//! The sequence itself lives in [`crate::git::merge_flow`] so this path and the
//! `robco_merge` MCP tool cannot drift apart. It races on the target
//! repository's `main` branch and working tree, which is why [`super::merge`]
//! serialises merges per repository in memory and the shared flow takes a
//! cross-process lock on top. It touches nothing outside that repository, which
//! is why merges in different repositories run concurrently.

use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use crate::{Result, config::MergeStrategy, git::merge_flow::MergeFlow};

pub(super) use crate::git::merge_flow::MergeMode;
/// Re-exported for the merge tests, which assert the labels the banner renders.
/// Nothing outside them names a step: the flow reports steps through the
/// callback, and the UI only ever stores what it is handed.
#[cfg(test)]
pub(super) use crate::git::merge_flow::{CLEANING_UP, MERGING_PR, PULLING_MAIN};

pub(super) const WORKER_TERMINATED: &str = "merge worker terminated unexpectedly";

/// The source recorded on the `MergeCompleted` runtime request, so the daemon
/// log distinguishes a merge the operator pressed from one an MCP caller asked
/// for.
const SOURCE: &str = "ui";

#[derive(Debug)]
pub(super) enum MergeEvent {
    Step(&'static str),
    Finished(std::result::Result<(), String>),
}

pub(super) struct MergeTarget {
    pub repo_path: PathBuf,
    pub branch: String,
    pub mode: MergeMode,
    pub strategy: MergeStrategy,
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
    MergeFlow {
        repo: &target.repo_path,
        branch: &target.branch,
        worktree: &target.worktree_path,
        tmux_session: &target.tmux_session,
        shell_session: &target.shell_session,
        mode: target.mode,
        strategy: target.strategy,
        source: SOURCE,
    }
    .run(|step| send_step(sender, step))
}

fn send_step(sender: &Sender<MergeEvent>, step: &'static str) {
    let _ = sender.send(MergeEvent::Step(step));
}
