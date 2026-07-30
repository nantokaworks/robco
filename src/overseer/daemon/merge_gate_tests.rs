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
        merge_hold_recheck_reason: None,
        merge_hold_recheck_head: None,
        merge_hold_stuck_notified: false,
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
    let plan = merge_recovery::plan(&mut e, &halt.reason, "sha", "base-sha", true, 2);
    assert_eq!(plan, merge_recovery::RecoveryPlan::Dispatch);
}

#[test]
fn an_unstable_pull_request_with_a_failed_check_holds_on_the_check_not_the_state() {
    // #349: `UNSTABLE` means "a check is not passing" — a less specific restatement
    // of what the rollup already says. Letting it preempt the checks stage the way
    // `DIRTY` must turns a worker-fixable `checks_not_green` into an operator-only
    // `merge_state:unstable`, which never reaches merge recovery. The rollup here
    // has a real run to read, unlike `DIRTY`'s permanently empty one, so the gate
    // must let the checks stage name it instead.
    let mut e = entry();
    let value = json!({
        "state": "OPEN",
        "mergeStateStatus": "UNSTABLE",
        "statusCheckRollup": [{"name": "ci", "conclusion": "FAILURE"}],
    });
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
    .expect("an unstable pull request with a failed check must be held");
    assert_eq!(halt.reason, "checks_not_green");

    // And that reason is the one `merge_recovery` already hands back to the worker —
    // proving the handback `merge_state:unstable` used to block now actually fires.
    use crate::overseer::daemon::merge_recovery;
    let plan = merge_recovery::plan(&mut e, &halt.reason, "sha", "base-sha", true, 2);
    assert_eq!(plan, merge_recovery::RecoveryPlan::Dispatch);
}

#[test]
fn an_unstable_pull_request_with_checks_still_running_waits_not_holds_on_the_state() {
    // The other direction of #349: an `UNSTABLE` head whose rollup has not finished
    // must wait on the checks, not report a merge-state reason that skips recovery
    // entirely — nothing has failed yet, so nothing should be handed to a worker.
    let mut e = entry();
    let value = json!({
        "state": "OPEN",
        "mergeStateStatus": "UNSTABLE",
        "statusCheckRollup": [{"name": "ci", "conclusion": null}],
    });
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
    .expect("an unstable pull request with running checks must still be held");
    assert_eq!(halt.reason, "checks_waiting");
}

#[test]
fn a_blocked_pull_request_with_a_green_rollup_still_holds_on_the_missing_review() {
    // #349: `BLOCKED` falls through to the checks stage now, but a green rollup
    // cannot explain a missing review — that fact lives outside `statusCheckRollup`
    // entirely. `merge_state_cleared` must still catch it and hold under the
    // state's own name, or a blocked pull request with passing checks would merge.
    let mut e = entry();
    let value = json!({
        "state": "OPEN",
        "mergeStateStatus": "BLOCKED",
        "statusCheckRollup": [{"name": "ci", "conclusion": "SUCCESS"}],
    });
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
    .expect("a blocked pull request must be held even with a green rollup");
    assert_eq!(halt.reason, "merge_state:blocked");
}

#[test]
fn a_blocked_pull_request_with_a_failed_check_holds_on_the_check_not_the_state() {
    // The other half of `BLOCKED`'s two causes: when the rollup itself carries the
    // failure, the checks stage names it directly instead of the less specific
    // `merge_state:blocked`.
    let mut e = entry();
    let value = json!({
        "state": "OPEN",
        "mergeStateStatus": "BLOCKED",
        "statusCheckRollup": [{"name": "ci", "conclusion": "FAILURE"}],
    });
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
    .expect("a blocked pull request with a failed check must be held");
    assert_eq!(halt.reason, "checks_not_green");
}

#[test]
fn a_draft_pull_request_with_a_green_rollup_still_holds_on_the_draft_state() {
    // #349: a draft's checks may well be green — that says nothing about whether
    // it is ready to merge, so `merge_state_cleared` must still hold it under
    // `merge_state:draft` rather than letting a green rollup merge a draft.
    let mut e = entry();
    let value = json!({
        "state": "OPEN",
        "mergeStateStatus": "DRAFT",
        "statusCheckRollup": [{"name": "ci", "conclusion": "SUCCESS"}],
    });
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
    .expect("a draft pull request must be held even with a green rollup");
    assert_eq!(halt.reason, "merge_state:draft");
}

#[test]
fn a_draft_pull_request_with_a_failed_check_holds_on_the_check_not_the_state() {
    // The other half of `DRAFT`'s fall-through: a failing check is named directly
    // so the worker still gets pushed the fix, exactly as it would once the pull
    // request is marked ready for review.
    let mut e = entry();
    let value = json!({
        "state": "OPEN",
        "mergeStateStatus": "DRAFT",
        "statusCheckRollup": [{"name": "ci", "conclusion": "FAILURE"}],
    });
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
    .expect("a draft pull request with a failed check must be held");
    assert_eq!(halt.reason, "checks_not_green");
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
