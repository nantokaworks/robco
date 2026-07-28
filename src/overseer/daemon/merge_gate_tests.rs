use serde_json::json;

use super::*;
use crate::overseer::{daemon::merge_recovery, ledger::LedgerPhase};

fn entry() -> LedgerEntry {
    LedgerEntry {
        task_id: "task".into(),
        display_id: "#1".into(),
        repo: "/repo".into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
        phase: LedgerPhase::PrOpened,
        dispatched_at: chrono::Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: Some("https://pr/1".into()),
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        manual_merge_skip: None,
        merge_judge_fail_safes: 0,
        merge_hold_cap_escalated: false,
        merge_hold_rechecks: 0,
    }
}

/// Config that skips the protection probe: these tests exercise the merge-state /
/// checks ordering, not protection, and `ProtectionMode::Off` is what lets `gate`
/// run to completion without shelling out to `gh api`.
fn config_without_protection() -> Config {
    let mut config = Config::default();
    config.overseer.protection_mode = crate::overseer::config::ProtectionMode::Off;
    config
}

#[test]
fn a_conflicting_pull_request_with_no_checks_holds_on_the_conflict_not_the_wait() {
    // #343: GitHub cannot construct `refs/pull/N/merge` for a head that conflicts
    // with its base, so a `DIRTY` pull request reports an empty check rollup — not
    // because nothing has run yet, but because nothing ever will. The gate must
    // hold this as `merge_state:dirty`, not `checks_waiting`, or the failure never
    // reaches merge recovery and the pull request spins until an operator notices.
    let mut e = entry();
    let value = json!({"state": "OPEN", "mergeStateStatus": "DIRTY", "statusCheckRollup": []});
    let halt = gate(
        &mut e,
        "https://pr/1",
        &value,
        &config_without_protection(),
        &mut ProtectionCache::default(),
        &Registry {
            version: 1,
            repos: vec![],
        },
        &mut merge_queue::Heads::new(),
    )
    .expect("a conflicting pull request must be held");
    assert_eq!(halt.reason, "merge_state:dirty");

    // The reason the gate now reports is one `merge_recovery` already knows how to
    // act on. Asserting that here is what proves the masked-reason half of #343 is
    // actually fixed, not just the label: a `checks_waiting` reason would have
    // classified as `FailureClass::Operator` and never reached this dispatch.
    let plan = merge_recovery::plan(&mut e, &halt.reason, "sha", true, 2);
    assert_eq!(plan, merge_recovery::RecoveryPlan::Dispatch);
}

#[test]
fn a_clean_pull_request_with_pending_checks_still_waits_on_the_checks() {
    // No regression in the ordinary case: a mergeable head with checks that simply
    // have not finished yet is still held as `checks_waiting`, not handed back —
    // there is nothing here for a worker to fix.
    let mut e = entry();
    let value = json!({"state": "OPEN", "mergeStateStatus": "CLEAN", "statusCheckRollup": []});
    let halt = gate(
        &mut e,
        "https://pr/1",
        &value,
        &config_without_protection(),
        &mut ProtectionCache::default(),
        &Registry {
            version: 1,
            repos: vec![],
        },
        &mut merge_queue::Heads::new(),
    )
    .expect("a pull request with no rollup yet must still be held");
    assert_eq!(halt.reason, "checks_waiting");
}

#[test]
fn allow_unverifiable_protection_waives_only_the_plan_unsupported_reason() {
    // #316: only `plan_unsupported` is waivable; every other reason still gates.
    use crate::overseer::daemon::protection as p;
    for (reason, allowed, expected) in [
        (p::PLAN_UNSUPPORTED, true, true),
        (p::PLAN_UNSUPPORTED, false, false),
        (p::NO_PULL_REQUEST_RULE, true, false),
        (p::NO_REQUIRED_STATUS_CHECKS, true, false),
        (p::PROBE_UNAVAILABLE, true, false),
        (p::UNKNOWN_REMOTE, true, false),
    ] {
        assert_eq!(protection_gate_overridden(reason, allowed), expected);
    }
}
