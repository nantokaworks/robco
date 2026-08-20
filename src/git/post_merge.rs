//! The post-merge cleanup sequence, shared by the interactive merge action and
//! the Overseer daemon.
//!
//! Both paths run the same steps in the same order — resolve the repository's
//! own default branch (dropr:503; never a hardcoded `main`), fetch it,
//! advance the primary checkout's local copy of it, remove the task worktree,
//! delete the local branch, delete the remote branch — and differ only in
//! what a failing step means. An interactive merge is watched, so it stops at
//! the first failure and reports it. The daemon is not watched, so it records
//! the failure and runs the remaining steps: a base branch that cannot be
//! resolved or fetched must not strand a worktree and a branch forever.
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
        let base = self.resolve_base(&mut outcome)?;
        if let Some(base) = &base {
            match git::fetch_branch(self.repo, base) {
                Ok(()) => outcome.base_pulled = self.advance_main(base, &mut outcome),
                Err(error) => {
                    self.record(&mut outcome, "fetching the base branch", error)?;
                }
            }
        }
        step(CleanupStep::CleaningUp);
        self.remove_worktree(base.as_deref(), &mut outcome)?;
        self.delete_branch(base.as_deref(), &mut outcome)?;
        // The remote branch is usually gone already — GitHub deletes it on merge
        // when the repository asks it to — so its absence is not a failure. The
        // call stays for repositories that do not, and is best-effort in both
        // paths.
        let _ = git::delete_remote_branch(self.repo, self.branch);
        Ok(outcome)
    }

    /// Resolves the repository's own default branch fresh, per run. Never a
    /// guessed name — an unresolved default branch is recorded like any
    /// other failed step, via [`Self::record`].
    fn resolve_base(&self, outcome: &mut CleanupOutcome) -> Result<Option<String>> {
        let resolved = match git::default_branch(self.repo) {
            Ok(Some(branch)) => Ok(branch),
            Ok(None) => Err(Error::Command {
                context: "git symbolic-ref refs/remotes/origin/HEAD",
                stderr: "origin/HEAD is not set; run `git remote set-head origin -a`".to_string(),
            }),
            Err(error) => Err(error),
        };
        match resolved {
            Ok(branch) => Ok(Some(branch)),
            Err(error) => {
                self.record(outcome, "resolving the base branch", error)?;
                Ok(None)
            }
        }
    }

    /// Advances the primary checkout's local `base` to `origin/<base>`, now
    /// that it has been fetched fresh. Returns whether it now matches.
    /// Two shapes are safe without a divergence check of their own:
    /// - The base branch is checked out with a clean tree: fast-forward the
    ///   checkout in place (`git merge --ff-only`), which refuses on its own
    ///   if the local branch is not an ancestor of the fetched ref.
    /// - Anything else is checked out: move only the branch ref
    ///   (`git fetch .`), which equally refuses a non-fast-forward, so a
    ///   diverged local base branch is left untouched either way.
    ///
    /// A checked-out base branch with a dirty tree is the one case neither
    /// shape covers safely, so it is skipped outright. Every skip is a note,
    /// never a failure: this step only ever leaves the checkout exactly as it
    /// was, so there is nothing for [`OnFailure::Abort`] to protect against.
    fn advance_main(&self, base: &str, outcome: &mut CleanupOutcome) -> bool {
        let base_ref = format!("origin/{base}");
        let advanced = match git::current_branch(self.repo) {
            Ok(Some(branch)) if branch == base => match git::worktree_is_clean(self.repo) {
                Ok(true) => git::fast_forward_checkout(self.repo, &base_ref)
                    .map_err(|error| format!("fast-forwarding {base} failed: {error}")),
                Ok(false) => Err(format!("{base} is checked out with uncommitted changes")),
                Err(error) => Err(format!("checking the {base} worktree failed: {error}")),
            },
            Ok(_) => git::fast_forward_ref(self.repo, base, &base_ref)
                .map_err(|error| format!("fast-forwarding the local {base} ref failed: {error}")),
            Err(error) => Err(format!("checking the checked-out branch failed: {error}")),
        };
        match advanced {
            Ok(()) => true,
            Err(reason) => {
                outcome
                    .notes
                    .push(self.main_behind_note(base, &base_ref, reason));
                false
            }
        }
    }

    /// Renders an `advance_main` skip reason, adding the commit count
    /// `base_ref` is now ahead by when it can be measured.
    fn main_behind_note(&self, base: &str, base_ref: &str, reason: String) -> String {
        match git::ahead_behind(self.repo, base, base_ref) {
            Ok((_, behind)) if behind > 0 => {
                format!("{reason}; {base} is behind {base_ref} by {behind}")
            }
            _ => reason,
        }
    }

    fn remove_worktree(&self, base: Option<&str>, outcome: &mut CleanupOutcome) -> Result<()> {
        if !self.worktree.exists() {
            outcome.worktree_removed = true;
            return Ok(());
        }
        match git::remove_worktree(self.repo, self.worktree, false) {
            Ok(()) => outcome.worktree_removed = true,
            Err(error) if is_dirty_worktree_error(&error) && self.branch_landed(base) => {
                // `self.branch_landed()` is what makes this safe regardless of
                // why the tree is dirty: a `git worktree remove` killed
                // mid-delete by a timeout leaves exactly this shape behind,
                // and plain removal then fails forever with the same
                // "use --force" error every retry (dropr:TKxgioWtorh7MZPbTBseC)
                // — but the check does not assume that is the reason. It only
                // forces past a dirty tree once the branch's own content is
                // provably already in the base, so there is nothing left in
                // the worktree that force-deleting could lose.
                match git::remove_worktree(self.repo, self.worktree, true) {
                    Ok(()) => outcome.worktree_removed = true,
                    Err(error) => self.record(outcome, "force-removing the worktree", error)?,
                }
            }
            Err(error) => self.record(outcome, "removing the worktree", error)?,
        }
        Ok(())
    }

    /// Whether `self.branch`'s content is already in `origin/<base>` — the
    /// same check [`Cleanup::delete_branch`] runs, reused here as the gate
    /// for forcing past a dirty tree. `base` unresolved, or the lookup
    /// itself failing, both answer `false`: never license to force-delete.
    fn branch_landed(&self, base: Option<&str>) -> bool {
        base.is_some_and(|base| {
            git::branch_content_merged(self.repo, self.branch, &format!("origin/{base}"))
                .unwrap_or(false)
        })
    }

    /// Deletes the local branch once its changes are provably in the base.
    ///
    /// The check is on content, not ancestry: this repository squash-merges, so
    /// the branch tip is not an ancestor of the base and `git branch -d` would
    /// refuse. Deleting with `-D` is only safe *because* the check ran, so a
    /// branch that fails it is kept rather than force-deleted anyway.
    fn delete_branch(&self, base: Option<&str>, outcome: &mut CleanupOutcome) -> Result<()> {
        if !outcome.worktree_removed {
            // No separate note here: `remove_worktree` already recorded why
            // it failed (the only way to reach this branch with
            // `worktree_removed` still false), and repeating "its worktree is
            // still checked out" alongside that note doubled every cleanup
            // failure into two decision-log lines per pass for no new
            // information.
            outcome.branch = BranchOutcome::Kept;
            return Ok(());
        }
        match git::branch_exists(self.repo, self.branch) {
            Ok(false) => return Ok(()),
            Ok(true) => {}
            Err(error) => {
                self.record(outcome, "looking up the branch", error)?;
                return self.keep(outcome, "the branch lookup failed");
            }
        }
        let Some(base) = base else {
            return self.keep(outcome, "the base branch could not be resolved");
        };
        match git::branch_content_merged(self.repo, self.branch, &format!("origin/{base}")) {
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

/// Whether `error` is `git worktree remove`'s refusal to delete a worktree
/// with modified or untracked files — the one shape [`Cleanup::remove_worktree`]
/// treats as safe to force past, rather than every possible removal failure
/// (a locked file, a permissions error, and so on stay unforced).
fn is_dirty_worktree_error(error: &Error) -> bool {
    matches!(
        error,
        Error::Command { stderr, .. } if stderr.contains("contains modified or untracked files")
    )
}

#[cfg(test)]
#[path = "post_merge_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "post_merge_worktree_removal_tests.rs"]
mod worktree_removal_tests;
