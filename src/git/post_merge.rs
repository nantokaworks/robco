//! The post-merge cleanup sequence, shared by the interactive merge action and
//! the Overseer daemon.
//!
//! Both paths run the same steps in the same order — fast-forward the primary
//! worktree, remove the task worktree, delete the local branch, delete the
//! remote branch — and differ only in what a failing step means. An interactive
//! merge is watched, so it stops at the first failure and reports it. The daemon
//! is not watched, so it records the failure and runs the remaining steps: a
//! `main` that cannot fast-forward must not strand a worktree and a branch
//! forever.

use std::path::Path;

use crate::{Error, Result, git};

/// The ref the branch's content is looked for in. The primary worktree tracks
/// the base branch and was just fast-forwarded, so its `HEAD` is the merge
/// commit. Reading the branch name from the repository instead would only add a
/// way to disagree with the worktree that was actually pulled.
const BASE: &str = "HEAD";

/// Progress markers, so a caller with a UI can name the running step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupStep {
    PullingMain,
    CleaningUp,
}

/// What a failing step means to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnFailure {
    /// Stop and return the error. For callers that report it to a human.
    Abort,
    /// Record the failure in [`CleanupOutcome::notes`] and run the rest of the
    /// sequence. For callers that log instead of reporting.
    Continue,
}

pub struct Cleanup<'a> {
    pub repo: &'a Path,
    pub worktree: &'a Path,
    pub branch: &'a str,
    pub on_failure: OnFailure,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct CleanupOutcome {
    pub worktree_removed: bool,
    pub branch: BranchOutcome,
    /// Failures and skipped steps, in the order they happened. Always empty
    /// under [`OnFailure::Abort`], which returns the first failure instead.
    pub notes: Vec<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub enum BranchOutcome {
    /// No local branch of that name — someone else already deleted it.
    #[default]
    Absent,
    Deleted,
    /// Left in place; [`CleanupOutcome::notes`] carries the reason.
    Kept,
}

impl Cleanup<'_> {
    /// Runs the sequence, reporting each step to `step` as it starts.
    pub fn run(&self, mut step: impl FnMut(CleanupStep)) -> Result<CleanupOutcome> {
        let mut outcome = CleanupOutcome::default();
        step(CleanupStep::PullingMain);
        if let Err(error) = git::pull_ff_only(self.repo) {
            self.record(&mut outcome, "fast-forwarding the primary worktree", error)?;
        }
        step(CleanupStep::CleaningUp);
        self.remove_worktree(&mut outcome)?;
        self.delete_branch(&mut outcome)?;
        // The remote branch is usually gone already — GitHub deletes it on merge
        // when the repository asks it to — so its absence is not a failure. The
        // call stays for repositories that do not, and is best-effort in both
        // paths.
        let _ = git::delete_remote_branch(self.repo, self.branch);
        Ok(outcome)
    }

    fn remove_worktree(&self, outcome: &mut CleanupOutcome) -> Result<()> {
        if !self.worktree.exists() {
            outcome.worktree_removed = true;
            return Ok(());
        }
        match git::remove_worktree(self.repo, self.worktree, false) {
            Ok(()) => outcome.worktree_removed = true,
            Err(error) => self.record(outcome, "removing the worktree", error)?,
        }
        Ok(())
    }

    /// Deletes the local branch once its changes are provably in the base.
    ///
    /// The check is on content, not ancestry: this repository squash-merges, so
    /// the branch tip is not an ancestor of the base and `git branch -d` would
    /// refuse. Deleting with `-D` is only safe *because* the check ran, so a
    /// branch that fails it is kept rather than force-deleted anyway.
    fn delete_branch(&self, outcome: &mut CleanupOutcome) -> Result<()> {
        if !outcome.worktree_removed {
            return self.keep(outcome, "its worktree is still checked out");
        }
        match git::branch_exists(self.repo, self.branch) {
            Ok(false) => return Ok(()),
            Ok(true) => {}
            Err(error) => {
                self.record(outcome, "looking up the branch", error)?;
                return self.keep(outcome, "the branch lookup failed");
            }
        }
        match git::branch_content_merged(self.repo, self.branch, BASE) {
            Ok(true) => {}
            Ok(false) => return self.keep(outcome, "its changes are not in the base branch"),
            Err(error) => {
                self.record(outcome, "checking whether the branch merged", error)?;
                return self.keep(outcome, "the merged check failed");
            }
        }
        match git::delete_branch(self.repo, self.branch) {
            Ok(()) => outcome.branch = BranchOutcome::Deleted,
            Err(error) => {
                self.record(outcome, "deleting the branch", error)?;
                return self.keep(outcome, "the delete failed");
            }
        }
        Ok(())
    }

    fn keep(&self, outcome: &mut CleanupOutcome, reason: &str) -> Result<()> {
        outcome.branch = BranchOutcome::Kept;
        outcome
            .notes
            .push(format!("branch {} kept: {reason}", self.branch));
        Ok(())
    }

    fn record(&self, outcome: &mut CleanupOutcome, context: &str, error: Error) -> Result<()> {
        match self.on_failure {
            OnFailure::Abort => Err(error),
            OnFailure::Continue => {
                outcome.notes.push(format!("{context} failed: {error}"));
                Ok(())
            }
        }
    }
}

#[cfg(test)]
#[path = "post_merge_tests.rs"]
mod tests;
