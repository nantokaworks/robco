//! How long the auto-merge gate may hold one pull request.
//!
//! Every exit of the gate that is not a merge names a reason and returns. The
//! caller records it, and the next pass — one poll interval later — reads the
//! same pull request, reaches the same exit, and records the same reason again.
//! For a condition that clears in minutes that is a running commentary; for one
//! that never clears it is the whole life of the pull request written to
//! `decisions.jsonl` one line at a time, with no counter, no age, and no phase
//! the board can show an operator.
//!
//! So a held pass costs budget. The (reason, head sha) pair is what the budget is
//! spent on: a new head is the worker's answer to the last one, and a changed
//! reason is a changed problem, so either restarts the count while a frozen pair
//! keeps spending it. A spent budget escalates the entry once and then goes
//! quiet, which is the same shape `merge_state::UPDATE_CAP_REACHED` and
//! `merge_recovery::CAP_REACHED` already have.
//!
//! Reasons that carry a budget of their own are left alone here, so one condition
//! is never charged twice.

use super::merge_decision::Halt;
use crate::overseer::{
    ledger::{LedgerEntry, MergeHold},
    logging::DecisionKind,
};

/// Reason recorded when the hold budget is spent. It carries the reason that ran
/// out, because "this pull request stopped moving" is only actionable together
/// with what it stopped on.
pub(super) fn cap_reached(reason: &str) -> String {
    format!("merge_hold_cap_reached:{reason}")
}

/// What to do about one held pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HoldPlan {
    /// Within budget: record the hold as the gate named it.
    Record,
    /// The budget is spent; the entry escalates to an operator.
    CapReached,
    /// This pair already escalated. Recording it again would restore exactly the
    /// repetition the budget exists to end.
    Spent,
}

/// Charges one held pass to `entry` and reports how to spend it.
///
/// The pass is charged before it is recorded, mirroring `merge_state::plan_update`
/// and `merge_recovery::plan`, so the budget describes passes spent rather than
/// decisions successfully written.
pub(super) fn charge(entry: &mut LedgerEntry, halt: &Halt, head: &str, max: u32) -> HoldPlan {
    if !bounded(halt) {
        return HoldPlan::Record;
    }
    let hold = &mut entry.merge_hold;
    if hold.reason.as_deref() != Some(halt.reason.as_str()) || hold.head.as_deref() != Some(head) {
        *hold = MergeHold {
            reason: Some(halt.reason.clone()),
            head: Some(head.to_owned()),
            ..MergeHold::default()
        };
    }
    if hold.escalated {
        return HoldPlan::Spent;
    }
    if hold.passes >= max {
        hold.escalated = true;
        return HoldPlan::CapReached;
    }
    hold.passes = hold.passes.saturating_add(1);
    HoldPlan::Record
}

/// Forgets what this entry was held on.
///
/// Called when the pass got past the deterministic gate — the pull request merged,
/// or its judgment is queued — so an entry that clears its hold and later meets a
/// new one starts from a full budget rather than from the old condition's residue.
pub(super) fn cleared(entry: &mut LedgerEntry) {
    entry.merge_hold = MergeHold::default();
}

/// Whether this exit is one the budget is responsible for bounding.
///
/// A skip is the gate reporting that there is nothing here for it to do — the
/// pull request has already settled — and it leaves the entry terminal, so it
/// cannot repeat. The `behind_` family is already bounded by
/// `overseer.max_branch_updates`, and charging it here would let one condition
/// spend two budgets and escalate under whichever ran out first.
fn bounded(halt: &Halt) -> bool {
    halt.kind != DecisionKind::Skip && !halt.reason.starts_with("behind_")
}

#[cfg(test)]
#[path = "merge_hold_tests.rs"]
mod tests;
