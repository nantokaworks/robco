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

use std::path::Path;

use serde_json::Value;

use super::merge_decision::{Halt, log};
use crate::{
    Result, git,
    overseer::{
        ledger::{LedgerEntry, MergeApproval},
        logging::DecisionKind,
    },
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
/// A head that no longer matches means a worker pushed after the operator
/// approved. That is not on its own reason to lose the approval (dropr:534):
/// `merge_recovery_enabled` exists precisely so a worker can repair a
/// failing check and push, and every successful recovery moves the head —
/// so without this, recovery invalidates the very approval it was serving.
/// [`carries_forward`] answers whether *this* push is that fix rather than
/// an unrelated one; when it is, the approval is re-granted against the new
/// head instead of consumed, so a merge failure this pass still leaves a
/// live approval for the next one. Every other case — a force-push, a
/// rebase, or a push robco never asked the worker to make — drops the
/// approval and records why, so it does not look, later, like a merge that
/// should already have happened.
pub(super) fn take_merge_approval(entry: &mut LedgerEntry, head: &str) -> Result<bool> {
    let Some(granted) = entry.merge_approval.take() else {
        return Ok(false);
    };
    if granted.head != head {
        if carries_forward(entry, &granted.head, head) {
            let reason = carried(&granted.head, head);
            entry.merge_approval = Some(MergeApproval {
                head: head.to_owned(),
                granted_at: granted.granted_at,
            });
            entry.approval_dropped = None;
            log(entry, DecisionKind::Merge, &reason, head)?;
            return Ok(true);
        }
        let reason = dropped(&granted.head, head);
        entry.approval_dropped = Some(reason.clone());
        log(entry, DecisionKind::Hold, &reason, head)?;
        return Ok(false);
    }
    entry.approval_dropped = None;
    log(entry, DecisionKind::Merge, "merge_approval", head)?;
    Ok(true)
}

/// Whether a push that moved the pull request past `approved_head` is the
/// specific fix robco itself asked the worker to make, rather than an
/// open-ended "any later commit" (dropr:534's own out-of-scope rail).
///
/// Two conditions both have to hold, and neither alone is enough:
///
/// - `entry.merge_recovery.head` still names `approved_head` — the exact
///   revision `merge_recovery::plan` last dispatched a handback for (see
///   `MergeRecovery::head`). A worker push unrelated to a robco-dispatched
///   recovery never sets this, so an approval never carries forward for a
///   push nobody asked the worker to make.
/// - `head` is a fast-forward of `approved_head` — a plain push, not a
///   force-push or a rebase, which would present a head the operator's
///   approval never covered even if recovery *did* just run.
fn carries_forward(entry: &LedgerEntry, approved_head: &str, head: &str) -> bool {
    entry.merge_recovery.head.as_deref() == Some(approved_head)
        && is_descendant(entry, approved_head, head)
}

/// Whether `head` is a fast-forward of `approved_head`, fetching
/// `entry.branch` fresh first: the daemon's primary checkout has no reason
/// to already hold a worker's latest push. A fetch failure, a missing
/// object, or any other read that does not answer cleanly resolves to
/// `false` — the safe default an ambiguous ancestry check must fall back to,
/// since the alternative is carrying an approval forward for a revision
/// nobody actually confirmed as the fix.
fn is_descendant(entry: &LedgerEntry, approved_head: &str, head: &str) -> bool {
    let repo = Path::new(&entry.repo);
    if git::fetch_branch(repo, &entry.branch).is_err() {
        return false;
    }
    git::is_ancestor(repo, approved_head, head).unwrap_or(false)
}

/// Reason recorded when a fast-forward push from robco's own recovery
/// handback keeps the operator's approval alive under its new head.
fn carried(approved_head: &str, head: &str) -> String {
    format!(
        "merge_approval_carried:{}..{}",
        short(approved_head),
        short(head)
    )
}

/// Reason recorded when a live request no longer names the pull request's
/// current head and does not qualify to carry forward.
fn dropped(approved_head: &str, head: &str) -> String {
    format!(
        "merge_approval_dropped:stale_head:{}..{}",
        short(approved_head),
        short(head)
    )
}

/// A short revision for the decision log — enough for an operator to look up
/// the commits added since their approval (`git log <short>..<short>`)
/// without the log line ballooning to two full 40-character shas.
fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

#[cfg(test)]
#[path = "merge_allow_tests.rs"]
mod tests;
