//! The read-only middle of the auto-merge gate: protection, merge state, checks,
//! and the merge state queue.
//!
//! Split out from `merge::evaluate` so a crafted [`Value`] can drive it directly —
//! every step here either reads `value` in memory or, for protection, probes an
//! endpoint that a `ProtectionMode::Off` config skips — which is what lets a test
//! exercise the ordering below without shelling out to `gh`.

use serde_json::Value;

use super::{
    merge_apply::merge_state_cleared,
    merge_decision::Halt,
    merge_queue, merge_state, protection,
    protection::ProtectionCache,
    pull_request::{Checks, base_branch, checks},
};
use crate::{config::Config, overseer::ledger::LedgerEntry, registry::Registry};

/// Reason recorded when a check finished and did not pass — the worker's own head is red.
const CHECKS_FAILED: &str = "checks_not_green";

/// Reason recorded when the checks have not finished. Nothing has failed, so nothing
/// is handed back.
const CHECKS_WAITING: &str = "checks_waiting";

/// Merge state is checked ahead of checks, not after: a pull request GitHub calls
/// decisively unmergeable (`DIRTY` at minimum) reports zero check runs, because
/// GitHub cannot construct `refs/pull/N/merge` for a conflicting head and so never
/// fires the checks that would run against it. Evaluating checks first would hold
/// such a pull request under `checks_waiting` forever — the actual, worker-fixable
/// conflict never gets a hold reason `merge_recovery` recognises. `Ready` and
/// `Behind` are unaffected: [`merge_state::held_reason`] returns `None` for both, so
/// they still reach the checks gate exactly as before, and `Behind`'s branch update
/// still only runs once checks are green, via `merge_state_cleared` below.
pub(super) fn gate(
    entry: &mut LedgerEntry,
    url: &str,
    value: &Value,
    config: &Config,
    cache: &mut ProtectionCache,
    registry: &Registry,
    heads: &mut merge_queue::Heads,
) -> Option<Halt> {
    let mode = config.overseer.protection_mode;
    if let Some(unmet) =
        protection::unmet_condition(entry, registry, cache, mode, base_branch(value))
        && !protection_gate_overridden(unmet, config.overseer.allow_unverifiable_protection)
    {
        return Some(Halt::gated(format!("unprotected:{unmet}")));
    }
    if let Some(reason) = merge_state::held_reason(value) {
        return Some(Halt::hold(reason));
    }
    match checks(value) {
        Checks::Green => {}
        Checks::Failed => return Some(Halt::hold(CHECKS_FAILED)),
        Checks::Waiting => return Some(Halt::hold(CHECKS_WAITING)),
    }
    merge_state_cleared(entry, url, value, config, heads)
}

/// Whether an `unprotected:*` reason should still gate the merge, given the
/// operator's `allow_unverifiable_protection` setting. Only `plan_unsupported` is
/// waivable — every other reason names a base branch the probe actually read and
/// found wanting, which this setting has no business overriding.
fn protection_gate_overridden(unmet: &str, allow_unverifiable_protection: bool) -> bool {
    unmet == protection::PLAN_UNSUPPORTED && allow_unverifiable_protection
}

#[cfg(test)]
#[path = "merge_gate_tests.rs"]
mod tests;
