//! The post-merge cleanup sequence, shared by the interactive merge action and
//! the Overseer daemon.
//!
//! Both paths run the same steps in the same order — fetch the base branch,
//! advance the primary checkout's local base branch, remove the task
//! worktree, delete the local branch, delete the remote branch — and differ
//! only in what a failing step means. An interactive merge is watched, so it
//! stops at the first failure and reports it. The daemon is not watched, so
//! it records the failure and runs the remaining steps: a base branch fetch
//! that fails must not strand a worktree and a branch forever.
//!
//! Advancing the local base branch is best-effort and never treated as a
//! failure of the sequence: it fast-forwards the checkout in place when the
//! base branch is checked out with a clean tree, moves only the branch ref
//! when something else is checked out (`git fetch` itself refuses a
//! non-fast-forward, so a diverged local base branch is left alone), and is
//! skipped — with a note — when the checked-out base branch is dirty. Nothing
//! else in this sequence touches the repository's own checked-out branch or
//! working tree — see [`git::fetch_branch`] — so the rest stays safe to run
//! against a checkout an operator or another process may be using at the same
//! time.

use std::path::Path;

use crate::{Error, Result, git};

/// Every merge in this repository lands on `main` — the same assumption
/// `agent.rs` and `overseer/daemon/pull_request.rs` make.
const BASE_BRANCH: &str = "main";

/// The ref the branch's content is looked for in: `origin/main`, fetched
/// fresh at the top of the sequence. Never the repository's own checked-out
/// branch — that may not even be `main`, and reading it would make this
/// depend on whatever an operator or another process left it on.
const BASE: &str = "origin/main";

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
    /// Whether the primary checkout's local base branch now matches
    /// `origin/<base>` — fetched, and either fast-forwarded in place or
    /// ref-updated, per [`Cleanup::advance_main`]. Reported separately from
    /// [`Self::notes`] because a caller has to be able to *act* on it: under
    /// [`OnFailure::Continue`] a failure here is only a note among the
    /// others, and the Overseer merge gate needs to know whether the base it
    /// is about to merge onto has actually caught up before lifting its
    /// settle barrier. A fetch failure still aborts under [`OnFailure::Abort`]
    /// rather than returning here — but a dirty or diverged checkout is never
    /// an abort, under either mode, since it never touched anything unsafe to
    /// leave alone; it only leaves `base_pulled` `false` and adds a note.
    pub base_pulled: bool,
    pub worktree_removed: bool,
    pub branch: BranchOutcome,
    /// Failures and skipped steps, in the order they happened. Empty under
    /// [`OnFailure::Abort`] unless the base branch fetch succeeded but the
    /// local base branch could not be advanced — that case is never an abort,
    /// see [`Self::base_pulled`].
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
        match git::fetch_branch(self.repo, BASE_BRANCH) {
            Ok(()) => outcome.base_pulled = self.advance_main(&mut outcome),
            Err(error) => {
                self.record(&mut outcome, "fetching the base branch", error)?;
            }
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

    /// Advances the primary checkout's local base branch to [`BASE`], now
    /// that it has been fetched fresh. Returns whether it now matches.
    ///
    /// Two shapes are safe without a divergence check of their own:
    /// - The base branch is checked out with a clean tree: fast-forward the
    ///   checkout in place (`git merge --ff-only`), which refuses on its own
    ///   if the local branch is not an ancestor of `BASE`.
    /// - Anything else is checked out: move only the branch ref
    ///   (`git fetch .`), which equally refuses a non-fast-forward, so a
    ///   diverged local base branch is left untouched either way.
    ///
    /// A checked-out base branch with a dirty tree is the one case neither
    /// shape covers safely, so it is skipped outright. Every skip is a note,
    /// never a failure: this step only ever leaves the checkout exactly as it
    /// was, so there is nothing for [`OnFailure::Abort`] to protect against.
    fn advance_main(&self, outcome: &mut CleanupOutcome) -> bool {
        let advanced = match git::current_branch(self.repo) {
            Ok(Some(branch)) if branch == BASE_BRANCH => match git::worktree_is_clean(self.repo) {
                Ok(true) => git::fast_forward_checkout(self.repo, BASE)
                    .map_err(|error| format!("fast-forwarding {BASE_BRANCH} failed: {error}")),
                Ok(false) => Err(format!(
                    "{BASE_BRANCH} is checked out with uncommitted changes"
                )),
                Err(error) => Err(format!(
                    "checking the {BASE_BRANCH} worktree failed: {error}"
                )),
            },
            Ok(_) => git::fast_forward_ref(self.repo, BASE_BRANCH, BASE).map_err(|error| {
                format!("fast-forwarding the local {BASE_BRANCH} ref failed: {error}")
            }),
            Err(error) => Err(format!("checking the checked-out branch failed: {error}")),
        };
        match advanced {
            Ok(()) => true,
            Err(reason) => {
                outcome.notes.push(self.main_behind_note(reason));
                false
            }
        }
    }

    /// Renders an `advance_main` skip reason, adding the commit count `BASE`
    /// is now ahead by when it can be measured — the number a `main behind
    /// origin/main by N` warning elsewhere is built from.
    fn main_behind_note(&self, reason: String) -> String {
        match git::ahead_behind(self.repo, BASE_BRANCH, BASE) {
            Ok((_, behind)) if behind > 0 => {
                format!("{reason}; {BASE_BRANCH} is behind {BASE} by {behind}")
            }
            _ => reason,
        }
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
