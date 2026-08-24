//! The merge approval robco is holding for an agent (dropr:545).
//!
//! Pressing `m` on a worktree whose pull request is not green does not merge
//! anything. It writes a `RuntimeRequest::MergeApproval` to the runtime-request
//! queue and ends. The daemon picks the request up on a later pass, so between
//! the keypress and that pass there is nothing in the ledger, and before
//! dropr:545 there was nothing on screen either — the operator could not tell
//! "robco accepted this" from "the key did nothing".
//!
//! This module holds the missing half. `queue_merge_approval` records the
//! approval here the moment `enqueue` succeeds, and the agent's tree row shows
//! a static `merge-queued` badge for as long as robco still holds the
//! approval.
//!
//! The badge is static on purpose. Queued is not running: the daemon may not
//! act for a whole poll interval, so animating it would claim work is
//! happening this instant when none is. Motion in robco's own vocabulary is
//! reserved for [`super::merge`], where a robco thread really is running
//! `git` and `gh`.
//!
//! See the dropr:545 decision scribble for the full rule and for what it
//! deliberately excludes.

use std::time::{Duration, Instant};

use crate::overseer::ledger::{Ledger, terminal};

use super::super::App;

/// One merge approval this process queued and the daemon has not confirmed
/// yet. Dropped once the ledger records the same approval, once the entry
/// stops, or once it ages out — see [`App::merge_approval_queued`].
pub(in crate::ui) struct QueuedApproval {
    at: Instant,
}

impl QueuedApproval {
    #[cfg(test)]
    pub(in crate::ui) fn aged(age: Duration) -> Self {
        Self {
            at: Instant::now() - age,
        }
    }
}

/// Poll intervals a local record may outlive the daemon before it stops
/// lighting the badge.
///
/// `runtime_request::enqueue` wakes the daemon straight away, so a healthy
/// drain lands about a second after the keypress and this bound is never
/// reached. It exists for the daemon that is down or wedged: the request
/// really is still queued, but a badge nobody can ever clear is not worth the
/// screen, and the OVERSEER header already reports a dead daemon.
const QUEUED_APPROVAL_POLLS: u64 = 3;

/// Floor for the bound above, for a configuration with a very short poll
/// interval.
const QUEUED_APPROVAL_FLOOR: Duration = Duration::from_secs(30);

/// Whether the ledger itself holds a live merge approval for `agent_id`.
/// This is the daemon's own record of the approval the operator queued, and
/// it takes over from the local record at the hand-off.
fn ledger_holds_approval(ledger: &Ledger, agent_id: &str) -> bool {
    ledger
        .entries
        .iter()
        .find(|entry| entry.agent_id == agent_id)
        .is_some_and(|entry| entry.merge_approval.is_some() && !terminal(entry.phase))
}

/// Whether the ledger entry for `agent_id` has stopped. A stopped entry ends
/// the local record: whatever the operator queued is over, merged or not.
fn ledger_entry_stopped(ledger: &Ledger, agent_id: &str) -> bool {
    ledger
        .entries
        .iter()
        .find(|entry| entry.agent_id == agent_id)
        .is_some_and(|entry| terminal(entry.phase))
}

impl App {
    /// Remembers that robco just queued a merge approval for `agent_id`.
    /// Called only on a successful enqueue — a failed one already shows its
    /// own error, and claiming robco is acting on a request that never
    /// reached the queue is exactly the lie this badge exists to end.
    pub(in crate::ui) fn note_merge_approval_queued(&mut self, agent_id: &str) {
        self.queued_merge_approvals
            .insert(agent_id.to_string(), QueuedApproval { at: Instant::now() });
    }

    /// Whether robco is holding a merge approval for `agent_id` right now.
    ///
    /// Reads two sources, either of which is enough: the ledger, once the
    /// daemon has taken the approval, and this process's own record, for the
    /// window before that. The read is pure — it never depends on
    /// [`App::prune_queued_merge_approvals`] having run — so a frame drawn
    /// between ticks still tells the truth.
    ///
    /// The local record deliberately ignores `entry.approval_dropped`: inside
    /// the hand-off window the daemon has not seen this approval yet, so any
    /// value there belongs to an earlier round.
    pub(in crate::ui) fn merge_approval_queued(&self, agent_id: &str) -> bool {
        let ledger = &self.overseer_snapshot.ledger;
        if ledger_holds_approval(ledger, agent_id) {
            return true;
        }
        self.queued_merge_approvals
            .get(agent_id)
            .is_some_and(|queued| {
                !ledger_entry_stopped(ledger, agent_id)
                    && queued.at.elapsed() <= self.queued_approval_limit()
            })
    }

    /// Drops local records the ledger has taken over, that have stopped, or
    /// that aged out. Hygiene only: the map would otherwise keep a row for
    /// every agent the operator ever pressed `m` on.
    pub(in crate::ui) fn prune_queued_merge_approvals(&mut self) {
        let limit = self.queued_approval_limit();
        let ledger = &self.overseer_snapshot.ledger;
        self.queued_merge_approvals.retain(|agent_id, queued| {
            !ledger_holds_approval(ledger, agent_id)
                && !ledger_entry_stopped(ledger, agent_id)
                && queued.at.elapsed() <= limit
        });
    }

    fn queued_approval_limit(&self) -> Duration {
        Duration::from_secs(
            self.overseer_snapshot
                .overseer
                .poll_interval_secs
                .saturating_mul(QUEUED_APPROVAL_POLLS),
        )
        .max(QUEUED_APPROVAL_FLOOR)
    }
}

#[cfg(test)]
#[path = "merge_queued_tests.rs"]
mod tests;
