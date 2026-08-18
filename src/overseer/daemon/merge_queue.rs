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
//!
//! The slot is given back when its holder leaves the queue *during* the pass
//! that holds it — see [`Heads::release`]. Without that, a repository whose
//! head merged early in a pass spent the rest of that pass with its slot held
//! by a pull request that is no longer in the queue, so the one now at the
//! head could not start catching up to the base until the next pass. The queue
//! is one action per repository at a time, not one action per repository per
//! pass.

use std::collections::{HashMap, hash_map::Entry};

/// Reason recorded for a pull request that is behind its base but is not next
/// in its repository's merge order. Kept apart from
/// `merge_state::BRANCH_UPDATED` so an operator reading the log can tell a
/// pull request waiting its turn from one whose branch was actually touched.
///
/// `pub(crate)` rather than `pub(super)` because `overseer::remedy` — outside
/// `daemon` — needs to name this exact reason too, to route it to `Move::Watch`
/// instead of falling through to the generic operator fallback.
pub(crate) const WAITING_TURN: &str = "behind_not_next";

/// Which repositories have already spent this pass's per-repository action
/// slot.
///
/// Built fresh in `auto_merge_pass` and threaded through every entry it
/// evaluates that pass — never stored on the ledger, because "who is head"
/// is recomputed from scratch every pass rather than remembered from the
/// last one.
///
/// Records *who* holds the slot, not merely that it is held. Only the holder
/// may give it back — see [`Heads::release`].
#[derive(Debug, Default)]
pub(super) struct Heads {
    /// Repository path to the agent id of the entry holding its action slot.
    ///
    /// Keyed on the agent id rather than the task id because only the agent id
    /// names one *entry*. A re-dispatched task pushes a second entry carrying
    /// the same `task_id` (`dispatch::worker::record_attempt`), and its old
    /// entry stays in the ledger with its pull request still open — so a task id
    /// would let one attempt release the slot its other attempt is holding.
    /// `observations::adopt_registry_children` dedupes entries on the agent id,
    /// and `discord_events` already treats it as the per-entry key.
    acting: HashMap<String, String>,
}

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
    pub(super) fn claim(&mut self, repo: &str, agent_id: &str) -> bool {
        match self.acting.entry(repo.to_owned()) {
            Entry::Occupied(_) => false,
            Entry::Vacant(slot) => {
                slot.insert(agent_id.to_owned());
                true
            }
        }
    }

    /// Whether `repo`'s head-of-queue slot is still open.
    ///
    /// Read rather than taken, by the one caller that has to decide whether an
    /// entry is worth a GitHub read *before* the gate reaches the claim — see
    /// the merge-settle barrier in `merge::auto_merge_pass`.
    pub(super) fn free(&self, repo: &str) -> bool {
        !self.acting.contains_key(repo)
    }

    /// Gives `repo`'s head-of-queue slot back, because the pull request holding
    /// it left the queue on this pass — it merged, or it escalated to an
    /// operator.
    ///
    /// The slot exists to stop two pull requests of one repository acting on a
    /// base only one of them can have. A holder that is gone cannot invalidate
    /// anything, so the pull request now at the head takes the slot in this
    /// same pass and starts its branch update a poll interval sooner.
    ///
    /// `agent_id` is checked against the recorded holder, and a call from anyone
    /// else is ignored. Most entries that reach a terminal phase during a pass
    /// never claimed at all — a pull request GitHub reports closed stops before
    /// the gate, and the hold cap escalates entries held on `checks_not_green`
    /// or `merge_state:dirty`, neither of which claims. Letting one of those
    /// free the slot would hand it to a third pull request while the real head
    /// was mid-branch-update, and both would spend a check run for a base only
    /// one of them can merge onto — the wasted CI this module exists to stop.
    pub(super) fn release(&mut self, repo: &str, agent_id: &str) {
        if self.acting.get(repo).is_some_and(|held| held == agent_id) {
            self.acting.remove(repo);
        }
    }
}

#[cfg(test)]
#[path = "merge_queue_tests.rs"]
mod tests;
