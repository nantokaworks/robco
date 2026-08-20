//! The recoverable-failure budget: whether one handback should be charged,
//! recorded as declined, or left alone. Split out of `merge_recovery.rs` to
//! keep that file under this project's source file size limit — `dispatch()`
//! and the delivery-confirmation handling that follows a charged plan stay in
//! the parent module, since they act on a plan rather than decide one.

use crate::overseer::{
    ledger::LedgerEntry,
    remedy::{FailureClass, classify},
};

/// Reason recorded when the per-entry handback budget is spent.
pub const CAP_REACHED: &str = "merge_recovery_cap_reached";

/// What to do about one recorded failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryPlan {
    /// The failure is an operator's, or this revision has already been handed
    /// back. Nothing is recorded: these are the steady states, and a decision per
    /// poll interval would bury the ones that mean something.
    Idle,
    /// Charge one handback and prompt the worker.
    Dispatch,
    /// The budget is spent; the entry escalates to an operator.
    CapReached,
    /// A failure the owning worker could have fixed, left alone because recovery
    /// is switched off. Recorded once per revision rather than acted on: whether
    /// to switch the setting on is the operator's call, and until this was
    /// counted the entire recoverable classification was invisible in the
    /// default configuration.
    Dropped,
}

/// Charges one handback to `entry` and reports how to spend it.
///
/// The attempt is charged before it runs, mirroring `merge_state::plan_update`,
/// so a handback that never reaches its worker still consumes budget instead of
/// retrying forever. A new head resets the deduplication — the worker pushed, so
/// the next failure is a genuinely new one — and so does a base that moved under
/// a stationary head: `merge_state:dirty` and `merge_state:blocked` are
/// properties of the (head, base) pair, not of the head alone, so a later merge
/// to the base branch that leaves this head conflicting again is just as
/// genuinely new a failure as a pushed fix would be. Neither ever resets the
/// budget, or a worker that pushes a broken fix each round — or a busy base that
/// keeps moving — would loop indefinitely.
pub fn plan(
    entry: &mut LedgerEntry,
    reason: &str,
    head_sha: &str,
    base_sha: &str,
    enabled: bool,
    max_recoveries: u32,
) -> RecoveryPlan {
    // Without a head sha there is no deduplication key, and a handback that
    // cannot be deduplicated would re-prompt the worker on every pass.
    if head_sha.is_empty() || classify(reason) == FailureClass::Operator {
        return RecoveryPlan::Idle;
    }
    if !enabled {
        return dropped(entry, head_sha, base_sha);
    }
    if entry.merge_recovery.head.as_deref() == Some(head_sha)
        && entry.merge_recovery.base.as_deref() == Some(base_sha)
    {
        return RecoveryPlan::Idle;
    }
    if entry.merge_recovery.charged >= max_recoveries {
        return RecoveryPlan::CapReached;
    }
    entry.merge_recovery.charged = entry.merge_recovery.charged.saturating_add(1);
    entry.merge_recovery.head = Some(head_sha.to_owned());
    entry.merge_recovery.base = Some(base_sha.to_owned());
    RecoveryPlan::Dispatch
}

/// Whether `plan` just charged this dispatch for the same head its previous
/// attempt could not confirm delivery for, rather than a genuinely new
/// failure. `plan` sets `merge_recovery.head` to the head this poll is
/// dispatching for right before calling `dispatch`; `undelivered_head` is
/// the head `merge_recovery::undelivered_cap_reached` last recorded a failed
/// confirm against, and `merge_recovery::refund` clears `merge_recovery.head`
/// on every failed confirm without touching it — so the two matching means
/// this exact (head, base) pair is being retried, not seen for the first
/// time.
pub(super) fn is_retry_of_undelivered(entry: &LedgerEntry) -> bool {
    entry.merge_recovery.head.is_some()
        && entry.merge_recovery.head == entry.merge_recovery.undelivered_head
}

/// Counts one failure the disabled setting left unhanded, at most once per
/// revision.
///
/// The deduplication key is the (head, base) pair the handback would have used,
/// so the cost is one decision per revision rather than one per poll — the same
/// shape the enabled path already has. The count is kept on the entry so
/// `robco status` can report the setting's consequence beside the
/// setting itself.
fn dropped(entry: &mut LedgerEntry, head_sha: &str, base_sha: &str) -> RecoveryPlan {
    if entry.merge_recovery.dropped_head.as_deref() == Some(head_sha)
        && entry.merge_recovery.dropped_base.as_deref() == Some(base_sha)
    {
        return RecoveryPlan::Idle;
    }
    entry.merge_recovery.dropped_head = Some(head_sha.to_owned());
    entry.merge_recovery.dropped_base = Some(base_sha.to_owned());
    entry.merge_recovery.dropped = entry.merge_recovery.dropped.saturating_add(1);
    RecoveryPlan::Dropped
}

/// Reason recorded for a recoverable failure nobody was handed, carrying the
/// failure verbatim so the log says what the setting cost.
pub(super) fn disabled(reason: &str) -> String {
    format!("merge_recovery_disabled:{reason}")
}

#[cfg(test)]
#[path = "merge_recovery_plan_tests.rs"]
mod tests;
