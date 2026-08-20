//! Confirming that the pull request the deterministic gate just cleared is
//! still the one an operator's live request named (dropr:500).
//!
//! `merge_repo_pass::run` only ever reaches `merge_evaluate::evaluate` for an
//! entry that already carries a live request (`merge_approval` or
//! `operator_override`), so by the time this module runs, some request
//! exists. What is left to confirm is that it still names the pull request's
//! *current* head: a worker can push a fix after the operator approved an
//! older revision, and that push must earn its own request rather than ride
//! on one granted for a head it has since moved past.

use serde_json::Value;

use super::merge_decision::{Halt, log};
use crate::{
    Result,
    overseer::{ledger::LedgerEntry, logging::DecisionKind},
};

/// Whether a live request still covers the pull request's current head.
pub(super) enum Judgment {
    Allow,
    /// No live request names the current head.
    Halt(Halt),
}

/// Whether `entry`'s pull request is still covered by the request that let
/// it reach the gate, now that the deterministic gate has cleared it.
pub(super) fn confirm_requested(entry: &mut LedgerEntry, value: &Value) -> Result<Judgment> {
    let head = crate::overseer::daemon::pull_request::head_sha(value);
    if take_operator_override(entry, head)? || take_merge_approval(entry, head)? {
        return Ok(Judgment::Allow);
    }
    // Reachable only when both requests were stale on the same pass — every
    // other case is caught by `merge_repo_pass::run`'s own request check
    // before the gate ever runs. `Halt::skip` keeps this out of the hold
    // budget: there is nothing here to reconsider until the operator asks
    // again.
    Ok(Judgment::Halt(Halt::skip("merge_request_stale")))
}

/// Consumes `entry.operator_override` if it is still live and its head
/// matches the pull request's current one.
///
/// Matching on the exact head is what keeps the request scoped to the
/// revision the operator actually approved (see `ledger::OperatorOverride`):
/// a later push presents a head the operator never saw, and that revision
/// must earn its own request. Taken (cleared) either way, matched or not: a
/// request granted for a head this pull request has since moved past is
/// spent, not saved for a revision it was never granted for.
pub(super) fn take_operator_override(entry: &mut LedgerEntry, head: &str) -> Result<bool> {
    let Some(granted) = entry.operator_override.take() else {
        return Ok(false);
    };
    if granted.head != head {
        return Ok(false);
    }
    log(entry, DecisionKind::Merge, "operator_override", head)?;
    Ok(true)
}

/// Consumes `entry.merge_approval` if it is still live and its head matches
/// the pull request's current one — the approval the TUI `m` key or
/// Discord's `!merge` queued (see
/// `discord::ledger_requests::LedgerRequest::Approve`). This is the request
/// an entry's own reconsideration reaches this arm to spend: the approval
/// also reset `merge_hold_recheck`
/// (`discord::ledger_requests::record_approval`), which is what let this
/// entry's escalated phase be looked at again in the first place.
///
/// Taken (cleared) either way, matched or not: a head that no longer matches
/// means the worker pushed after the operator approved, and the drop is
/// recorded so it does not look, later, like a merge that should already
/// have happened.
pub(super) fn take_merge_approval(entry: &mut LedgerEntry, head: &str) -> Result<bool> {
    let Some(granted) = entry.merge_approval.take() else {
        return Ok(false);
    };
    if granted.head != head {
        log(
            entry,
            DecisionKind::Hold,
            "merge_approval_dropped:stale_head",
            head,
        )?;
        return Ok(false);
    }
    log(entry, DecisionKind::Merge, "merge_approval", head)?;
    Ok(true)
}

#[cfg(test)]
#[path = "merge_allow_tests.rs"]
mod tests;
