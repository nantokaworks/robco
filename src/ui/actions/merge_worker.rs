//! The off-thread half of a merge: the `git` / `gh` sequence run for one
//! repository, and the events it reports back to the UI thread.
//!
//! The sequence races on the target repository's `main` branch and working
//! tree, which is why [`super::merge`] serialises merges per repository. It
//! touches nothing outside that repository, which is why merges in different
//! repositories run concurrently.

use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use crate::{
    Result,
    config::MergeStrategy,
    git::{
        self,
        post_merge::{Cleanup, CleanupStep, OnFailure},
    },
    overseer::runtime_request::{self, RuntimeRequest},
};

pub(super) const MERGING_PR: &str = "merging PR";
pub(super) const PULLING_MAIN: &str = "pulling main";
pub(super) const CLEANING_UP: &str = "cleaning up";
pub(super) const WORKER_TERMINATED: &str = "merge worker terminated unexpectedly";

#[derive(Debug)]
pub(super) enum MergeEvent {
    Step(&'static str),
    Finished(std::result::Result<(), String>),
}

/// Whether the merge itself is still outstanding. A pull request that already
/// merged needs everything the sequence does *after* `gh pr merge` and must
/// never be handed to `gh pr merge` again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) enum MergeMode {
    MergeThenClean,
    CleanOnly,
}

impl MergeMode {
    /// The step the job starts on, so the progress banner does not open on
    /// "merging PR" for a run that never merges.
    pub(in crate::ui) fn first_step(self) -> &'static str {
        match self {
            Self::MergeThenClean => MERGING_PR,
            Self::CleanOnly => PULLING_MAIN,
        }
    }
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
    if target.mode == MergeMode::MergeThenClean {
        git::merge_pr(&target.repo_path, &target.branch, target.strategy)?;
    }
    // A watched merge stops at the first failure so the user sees it on the
    // banner; the Overseer daemon runs the same steps under `Continue`.
    Cleanup {
        repo: &target.repo_path,
        worktree: &target.worktree_path,
        branch: &target.branch,
        on_failure: OnFailure::Abort,
    }
    .run(|step| send_step(sender, step_label(step)))?;
    let _ = crate::tmux::kill_session(&target.tmux_session);
    let _ = crate::tmux::kill_session(&target.shell_session);
    announce_merge(&target.repo_path);
    Ok(())
}

/// Tell the Overseer daemon this repository just merged, so it reconciles the
/// merge on a pass that starts now instead of rediscovering it up to a poll
/// interval later. Announced after cleanup rather than straight after the
/// merge: the daemon cleans merged workers up too, and waking it mid-cleanup
/// would have both processes removing the same worktree and killing the same
/// sessions. Best effort — failing to announce only costs the delay this
/// removes, and there is no way to surface an error from a worker thread that
/// would not corrupt the TUI. A cleanup that fails aborts before this point, so
/// that merge is left for the daemon to find by polling, exactly as before.
fn announce_merge(repo_path: &Path) {
    let _ = runtime_request::enqueue(RuntimeRequest::MergeCompleted {
        source: "ui".into(),
        repo: repo_path.display().to_string(),
        at: chrono::Utc::now(),
    });
}

fn step_label(step: CleanupStep) -> &'static str {
    match step {
        CleanupStep::PullingMain => PULLING_MAIN,
        CleanupStep::CleaningUp => CLEANING_UP,
    }
}

fn send_step(sender: &Sender<MergeEvent>, step: &'static str) {
    let _ = sender.send(MergeEvent::Step(step));
}
