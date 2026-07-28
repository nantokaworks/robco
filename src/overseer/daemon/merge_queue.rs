//! Which pull request in a repository is next to merge.
//!
//! `merge_settle` already serialises the merge itself to one repository at a
//! time, but nothing gated the step *before* a merge: every pull request the
//! auto-merge pass found `BEHIND` had its branch updated in the same pass,
//! whether or not it was actually next in line. With N mergeable pull requests
//! open in one repository that is N-1 branch updates — and N-1 full CI
//! re-runs — thrown away the moment the one ahead of them merges.
//!
//! This module answers "is this pull request the one to act on this pass?"
//! for one repository at a time. The order itself is simply iteration order:
//! `auto_merge_pass` already walks the ledger's entries once per pass, new
//! entries are only ever appended, and pruning a settled one (`daemon::retention`)
//! leaves every survivor's relative order alone — so a repository's own entries
//! keep a stable order pass over pass, and the first one the pass reaches is
//! that repository's head of queue for this pass. Nothing here needs to be
//! persisted across passes: a repository starts every pass with an empty
//! claim, and whichever entry reaches the check first — because the ones
//! ahead of it merged, failed, or were skipped — claims it fresh.

use std::collections::HashSet;

/// Reason recorded for a pull request that is behind its base but is not next
/// in its repository's merge order. Kept apart from
/// `merge_state::BRANCH_UPDATED` so an operator reading the log can tell a
/// pull request waiting its turn from one whose branch was actually touched.
///
/// `pub(crate)` rather than `pub(super)` because `overseer::remedy` — outside
/// `daemon` — needs to name this exact reason too, to route it to `Move::Watch`
/// instead of falling through to the generic operator fallback.
pub(crate) const WAITING_TURN: &str = "behind_not_next";

/// Repositories that have already claimed this pass's one action slot.
///
/// Built fresh in `auto_merge_pass` and threaded through every entry it
/// evaluates that pass — never stored on the ledger, because "who is head"
/// is recomputed from scratch every pass rather than remembered from the
/// last one.
#[derive(Debug, Default)]
pub(super) struct Heads(HashSet<String>);

impl Heads {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Claims the head-of-queue slot for `repo`, reporting whether this call
    /// is the one that got it.
    ///
    /// Called for a pull request whose merge state is `Ready` or `Behind` —
    /// either verdict names one that is genuinely progressing, so whichever
    /// one reaches this first this pass is the repository's head and every
    /// other pull request of the same repository gains nothing from acting
    /// this pass too: the head merging (now or once its branch catches up)
    /// invalidates that work before it can be used.
    ///
    /// A `Held` verdict never calls this: a pull request blocked, failing, or
    /// otherwise stuck is not making progress, so letting it occupy the slot
    /// would stall every pull request behind it for as long as it stays
    /// stuck. Skipping the call is what lets the order pass over it instead.
    pub(super) fn claim(&mut self, repo: &str) -> bool {
        self.0.insert(repo.to_owned())
    }
}

#[cfg(test)]
#[path = "merge_queue_tests.rs"]
mod tests;
