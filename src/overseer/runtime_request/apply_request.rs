//! What applying each [`RuntimeRequest`] variant actually does to the ledger.
//!
//! Split out of `runtime_request.rs` to keep that file — the queue's own
//! read/write/quarantine machinery — under this project's file-size limit.

use std::collections::HashSet;

use chrono::Utc;

use super::RuntimeRequest;
use crate::{
    overseer::{
        command::escalate_workers,
        daemon::pull_request,
        discord::ledger_requests::record_runtime_approval,
        ledger::{self, Ledger, OperatorOverride},
    },
    registry::Registry,
};

pub(super) fn apply(ledger: &mut Ledger, request: RuntimeRequest, registry: Option<&Registry>) {
    match request {
        RuntimeRequest::PanicEscalate { agent_ids, .. } => {
            escalate_workers(ledger, &agent_ids.into_iter().collect::<HashSet<_>>());
        }
        // Nothing to apply: the pass that drains this request goes on to
        // observe the merge for itself, which is the whole point of waking it.
        RuntimeRequest::MergeCompleted { .. } => {}
        RuntimeRequest::OperatorMergeOverride { target, .. } => {
            grant_operator_override(ledger, &target, registry);
        }
        RuntimeRequest::MergeApproval {
            source,
            target,
            head,
            ..
        } => {
            record_runtime_approval(ledger, &target, &head, &source, registry);
        }
        // `drain_in` pulls this variant out before calling `apply` — see the
        // variant's own doc comment. Kept as an explicit no-op arm, not a
        // wildcard, so a future caller that bypasses `drain_in` fails safe
        // (nothing dispatches) instead of silently matching a wildcard.
        RuntimeRequest::RunTask { .. } => {}
        RuntimeRequest::BranchUpdated { target, .. } => reset_branch_updates(ledger, &target),
    }
}

/// Finds the ledger entry `target` names and resets what it remembers about
/// the automated branch-update budget, since an operator-driven update just
/// did on GitHub's side exactly what that budget tracks.
///
/// Reviving an `Escalated` entry back to `PrOpened` is safe even when the
/// escalation had nothing to do with falling behind: `merge_approval` (the
/// request that let the entry reach the gate in the first place) is never
/// cleared by escalation, so the entry is simply looked at again on the next
/// pass — and `merge_allow::take_merge_approval` already drops a stale
/// approval by itself once the update changes the head, rather than merging
/// on an operator's word alone.
///
/// A no-op when no entry names `target` at all: the branch may belong to an
/// agent the daemon never dispatched, which has nothing here to reset.
fn reset_branch_updates(ledger: &mut Ledger, target: &str) {
    let Some(entry) = ledger
        .entries
        .iter_mut()
        .find(|entry| entry.agent_id == target || entry.display_id == target)
    else {
        return;
    };
    entry.branch_updates = 0;
    if entry.phase == ledger::LedgerPhase::Escalated {
        entry.phase = ledger::LedgerPhase::PrOpened;
        entry.worker_escalated = false;
    }
}

/// Finds the ledger entry `target` names and records an operator override
/// scoped to its pull request's current head.
///
/// An already-existing entry with no pull request recorded yet is left
/// untouched rather than run through `ledger::ensure_landable`: there is
/// nothing an override can grant against it, so reviving a `Failed` or
/// `Escalated` settlement here would only discard that state for no gain.
/// `ensure_landable` still adopts or revives once a pull request is known —
/// or the entry does not exist at all yet, which `robco_approve` already
/// validates against before this request is ever enqueued (dropr:523).
///
/// Silently a no-op when there is truly nothing to adopt, or GitHub cannot be
/// read right now — an operator override that missed its moment is not a
/// failure this drain should abort or retry over, the same way [`apply`]
/// already treats a `PanicEscalate` naming a since-vanished agent id. The
/// operator can simply approve again.
fn grant_operator_override(ledger: &mut Ledger, target: &str, registry: Option<&Registry>) {
    let already_known_without_a_pr = ledger
        .entries
        .iter()
        .find(|entry| entry.agent_id == target || entry.display_id == target)
        .is_some_and(|entry| entry.pr_url.is_none());
    if already_known_without_a_pr {
        return;
    }
    let Some(entry) = ledger::ensure_landable(ledger, target, registry, Utc::now()) else {
        return;
    };
    let Some(url) = entry.pr_url.clone() else {
        return;
    };
    let Ok(value) = pull_request::read(&entry.repo, &url) else {
        return;
    };
    entry.operator_override = Some(OperatorOverride {
        head: pull_request::head_sha(&value).to_owned(),
        granted_at: Utc::now(),
    });
}
