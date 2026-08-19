//! Merge-state handling for the auto-merge gate.
//!
//! GitHub reports whether a pull request may be merged right now through
//! `mergeStateStatus`. The state that matters most to Overseer is `BEHIND`: merging one
//! pull request advances the base branch, which leaves every other open pull request in
//! that repository missing the new base commit. A base branch whose ruleset sets
//! `strict_required_status_checks_policy` refuses such a merge, so without updating the
//! branch the queue stalls after the first merge of each repository.
//!
//! `BEHIND` is therefore recoverable — the branch is updated and the pull request
//! returns to the queue so its required checks re-run against the new head — while every
//! other non-mergeable state is parked with a reason naming the state itself.
//!
//! Naming the state is what makes some of those other states recoverable too, just one
//! layer up: `overseer::remedy::classify` reads the reason this module records and, for
//! `merge_state:dirty` and `merge_state:blocked`, hands it back to the owning worker
//! through `merge_recovery` — a conflict or a missing review is exactly the kind of thing
//! a worker fixes by pushing. This module only decides how to name a state; whether that
//! name is ever acted on is `remedy`'s call, not this one's.
//!
//! Not every held state should preempt the checks stage, though — only `DIRTY` has a
//! head GitHub structurally never runs checks against (see
//! [`preempting_hold_reason`]). `BLOCKED`, `DRAFT`, and `UNSTABLE` all still have a head
//! checks genuinely run against, so `merge_gate::gate` lets them fall through to the
//! checks stage first, and relies on [`super::merge_apply::merge_state_cleared`] to hold
//! them under their own name afterward for the one case a green rollup cannot explain.
//! `BLOCKED` in particular can mean either a missing review or a missing required check:
//! falling through means a failing required check is named `checks_not_green` — the
//! more specific, already-recoverable reason — while a missing review alone (a green
//! rollup) still surfaces as `merge_state:blocked` once `merge_state_cleared` re-reads
//! it. `DRAFT` follows the same reasoning: a draft with failing checks is still named
//! `checks_not_green` so the worker gets pushed the fix, while a draft with a green
//! rollup surfaces as `merge_state:draft` so an operator knows to mark it ready.

use std::process::Command;

use serde_json::Value;

use super::COMMAND_TIMEOUT;
use crate::{
    config::{Config, MergeStrategy},
    overseer::{exec::run_timeout, ledger::LedgerEntry},
};

/// Reason recorded when a branch was updated onto its base. The pull request is held
/// rather than merged, because its required checks must re-run against the new head.
pub(super) const BRANCH_UPDATED: &str = "behind_branch_updated";

/// Reason recorded when the per-entry update budget is spent. `pub(crate)`
/// rather than `pub(super)` because `overseer::remedy` — outside `daemon` —
/// needs to name this exact reason too, to keep its own classification in
/// sync with `merge_recovery::classify`'s.
pub(crate) const UPDATE_CAP_REACHED: &str = "behind_update_cap_reached";

/// What `mergeStateStatus` means for this pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MergeState<'a> {
    /// GitHub accepts a merge now.
    Ready,
    /// The head is missing base commits and must be updated first.
    Behind,
    /// Not mergeable; the borrowed value is the raw state, which names why.
    Held(&'a str),
}

/// Classifies the pull request's `mergeStateStatus`.
///
/// An absent field is treated as `Ready`: the surrounding gate has already verified base
/// protection and green checks, and refusing on a field GitHub did not report would park
/// every pull request instead of merging it.
pub(super) fn merge_state(value: &Value) -> MergeState<'_> {
    let Some(raw) = value
        .get("mergeStateStatus")
        .and_then(Value::as_str)
        .filter(|state| !state.is_empty())
    else {
        return MergeState::Ready;
    };
    match raw {
        // `HAS_HOOKS` is mergeable; it only signals that pre-receive hooks will run.
        "CLEAN" | "HAS_HOOKS" => MergeState::Ready,
        "BEHIND" => MergeState::Behind,
        // `BLOCKED`, `DIRTY`, `DRAFT`, `UNSTABLE`, `UNKNOWN`, and any state GitHub adds
        // later each keep their own name so `decisions.jsonl` explains the hold.
        _ => MergeState::Held(raw),
    }
}

/// Hold reason for a state the gate cannot act on, e.g. `merge_state:dirty`.
pub(super) fn hold_reason(raw: &str) -> String {
    format!("merge_state:{}", raw.to_ascii_lowercase())
}

/// The hold reason for a merge state whose check rollup cannot be trusted to name the
/// real blocker — the only state this returns `Some` for is `DIRTY`.
///
/// Callers evaluate this *before* the checks gate: `DIRTY` reports zero check runs,
/// because GitHub cannot construct `refs/pull/N/merge` to run them against for a head
/// that conflicts with its base — not because nothing has started, but because nothing
/// ever will. Waiting on checks that can never exist would hold the pull request under
/// `checks_waiting` and mask a conflict the owning worker could otherwise be handed
/// back — see `overseer::remedy::classify`.
///
/// Every other held state (`BLOCKED`, `DRAFT`, `UNSTABLE`, `UNKNOWN`, and anything
/// GitHub adds later) still has a head checks genuinely run against, so this returns
/// `None` for them and lets `merge_gate::gate` send them through the checks stage
/// instead — see the module doc for why that loses no information.
pub(super) fn preempting_hold_reason(value: &Value) -> Option<String> {
    match merge_state(value) {
        MergeState::Held(raw) if raw == "DIRTY" => Some(hold_reason(raw)),
        MergeState::Held(_) | MergeState::Ready | MergeState::Behind => None,
    }
}

/// What to do about a pull request that has fallen behind its base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BehindPlan {
    /// Update the branch, optionally with the flag carried here.
    Update(Option<&'static str>),
    /// The update budget is spent; the entry escalates to an operator.
    Escalate,
}

/// Charges one branch update to `entry` and reports how to spend it.
///
/// The attempt is charged before it runs, so an update that fails still consumes budget
/// and a branch that can never be updated escalates instead of retrying forever.
pub(super) fn plan_update(entry: &mut LedgerEntry, config: &Config) -> BehindPlan {
    if entry.branch_updates >= config.overseer.max_branch_updates {
        return BehindPlan::Escalate;
    }
    entry.branch_updates = entry.branch_updates.saturating_add(1);
    BehindPlan::Update(update_flag(config.merge_strategy))
}

/// Keeps the update consistent with the configured merge strategy: a repository that
/// merges by rebase gets a rebased branch rather than a merge commit from the base.
/// A merge commit here is exactly what would later make the rebase merge itself
/// impossible, so the two settings cannot be allowed to drift apart.
fn update_flag(strategy: MergeStrategy) -> Option<&'static str> {
    (strategy == MergeStrategy::Rebase).then_some("--rebase")
}

/// Runs `gh pr update-branch`, returning the hold reason when it does not succeed.
pub(super) fn run_update(repo: &str, url: &str, flag: Option<&'static str>) -> Result<(), String> {
    let mut command = Command::new("gh");
    command
        .current_dir(repo)
        .args(["pr", "update-branch", url])
        .args(flag);
    match run_timeout(command, COMMAND_TIMEOUT) {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(format!("behind_update_exit:{}", output.status)),
        Err(error) => Err(format!("behind_update_error:{error}")),
    }
}

#[cfg(test)]
#[path = "merge_state_tests.rs"]
mod tests;
