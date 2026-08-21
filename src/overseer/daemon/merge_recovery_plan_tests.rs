use super::*;
use crate::overseer::ledger::{LedgerPhase, MergeRecovery};

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
        merge_hold_cap_escalated: false,
        merge_hold_rechecks: 0,
        merge_hold_recheck_reason: None,
        merge_hold_recheck_head: None,
        prerequisite_wait: None,
        merge_hold_stuck_notified: false,
        escalation_notified_reason: None,
        escalation_notified_head: None,
        worker_escalated: false,
        operator_override: None,
        merge_approval: None,
        pr_facts: None,
        worker_finished_at: None,
        approval_dropped: None,
    }
}

/// Switched off, the classification was inert *and* invisible: nothing was handed
/// back and nothing said so, which is why an operator reading `merge-recovery:
/// off` had no way to tell what the setting was costing them.
#[test]
fn a_disabled_recovery_records_the_failure_it_did_not_hand_back() {
    let mut entry = entry();
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-1", "base-1", false, 2),
        RecoveryPlan::Dropped
    );
    assert_eq!(entry.merge_recovery.dropped, 1);
    assert_eq!(entry.merge_recovery.dropped_head.as_deref(), Some("sha-1"));
    assert_eq!(entry.merge_recovery.dropped_base.as_deref(), Some("base-1"));
    // Nothing is charged and no worker is chosen: the switch still decides
    // whether anything happens, only the silence is gone.
    assert_eq!(entry.merge_recovery.charged, 0);
    assert_eq!(entry.merge_recovery.head, None);

    // One decision per revision, not one per poll. A new head is a new failure.
    for reason in ["merge_state:dirty", "checks_not_green"] {
        assert_eq!(
            plan(&mut entry, reason, "sha-1", "base-1", false, 2),
            RecoveryPlan::Idle
        );
    }
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-2", "base-1", false, 2),
        RecoveryPlan::Dropped
    );
    assert_eq!(entry.merge_recovery.dropped, 2);

    // A base that moved under the same head is a new failure too, the same way
    // it is when recovery is switched on.
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-2", "base-2", false, 2),
        RecoveryPlan::Dropped
    );
    assert_eq!(entry.merge_recovery.dropped, 3);
}

/// The drop is a consequence of the setting, so a failure no worker could fix
/// stays silent whether recovery is on or off.
#[test]
fn a_disabled_recovery_records_nothing_for_an_operator_only_failure() {
    let mut entry = entry();
    for reason in [
        "unprotected:unknown_remote",
        "checks_waiting",
        "missing_pr_url",
    ] {
        assert_eq!(
            plan(&mut entry, reason, "sha-1", "base-1", false, 2),
            RecoveryPlan::Idle
        );
    }
    assert_eq!(entry.merge_recovery, MergeRecovery::default());
}

/// The drops the disabled setting accumulated are what `robco status`
/// reports next to the switch, terminal entries included: an entry that escalated
/// because nobody was handed its failure is the case worth reading.
#[test]
fn the_ledger_totals_the_failures_the_switch_dropped() {
    let with_drops = |dropped: u32, phase| LedgerEntry {
        phase,
        merge_recovery: MergeRecovery {
            dropped,
            ..MergeRecovery::default()
        },
        ..entry()
    };
    let ledger = crate::overseer::ledger::Ledger {
        entries: vec![
            with_drops(2, LedgerPhase::PrOpened),
            with_drops(0, LedgerPhase::PrOpened),
            with_drops(3, LedgerPhase::Escalated),
        ],
        ..Default::default()
    };

    assert_eq!(ledger.merge_recovery_drops(), 5);
}

#[test]
fn an_operator_only_failure_is_never_charged() {
    let mut entry = entry();
    assert_eq!(
        plan(
            &mut entry,
            "unprotected:unknown_remote",
            "sha-1",
            "base-1",
            true,
            2
        ),
        RecoveryPlan::Idle
    );
    assert_eq!(entry.merge_recovery.charged, 0);
}

#[test]
fn the_same_failure_on_the_same_head_is_handed_back_once() {
    let mut entry = entry();
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-1", "base-1", true, 2),
        RecoveryPlan::Dispatch
    );
    assert_eq!(entry.merge_recovery.charged, 1);
    // The next poll interval finds the same revision failing the same way against
    // the same base; the worker is already working on it.
    for reason in ["merge_state:dirty", "checks_not_green"] {
        assert_eq!(
            plan(&mut entry, reason, "sha-1", "base-1", true, 2),
            RecoveryPlan::Idle
        );
    }
    assert_eq!(entry.merge_recovery.charged, 1);
}

#[test]
fn a_new_head_resets_the_dedupe_but_never_the_budget() {
    let mut entry = entry();
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-1", "base-1", true, 2),
        RecoveryPlan::Dispatch
    );
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-2", "base-1", true, 2),
        RecoveryPlan::Dispatch
    );
    assert_eq!(entry.merge_recovery.charged, 2);
    // A worker that pushes a broken fix each round would otherwise loop forever.
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-3", "base-1", true, 2),
        RecoveryPlan::CapReached
    );
    assert_eq!(entry.merge_recovery.charged, 2);
    assert_eq!(entry.merge_recovery.head.as_deref(), Some("sha-2"));
    assert_eq!(entry.merge_recovery.base.as_deref(), Some("base-1"));
}

/// #368: the live incident this test exists to catch. A pull request was
/// mergeable when its handback was delivered, and the same head then conflicted
/// again only because a later merge advanced the base branch — the worker never
/// touched anything. The dedup key must not mistake a stationary head for a
/// steady state when the base underneath it moved.
#[test]
fn a_moved_base_resets_the_dedupe_but_never_the_budget() {
    let mut entry = entry();
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-1", "base-1", true, 2),
        RecoveryPlan::Dispatch
    );
    // The head never moved, but a later merge to the base branch left it
    // conflicting again — a genuinely new failure the worker was never told
    // about.
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-1", "base-2", true, 2),
        RecoveryPlan::Dispatch
    );
    assert_eq!(entry.merge_recovery.charged, 2);
    // A busy base that keeps moving would otherwise re-arm the handback forever.
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-1", "base-3", true, 2),
        RecoveryPlan::CapReached
    );
    assert_eq!(entry.merge_recovery.charged, 2);
    assert_eq!(entry.merge_recovery.head.as_deref(), Some("sha-1"));
    assert_eq!(entry.merge_recovery.base.as_deref(), Some("base-2"));
}

#[test]
fn a_zero_budget_escalates_without_ever_prompting() {
    let mut entry = entry();
    assert_eq!(
        plan(&mut entry, "merge_error:x", "sha-1", "base-1", true, 0),
        RecoveryPlan::CapReached
    );
    assert_eq!(entry.merge_recovery.charged, 0);
}

#[test]
fn a_failure_without_a_head_sha_is_left_alone() {
    // Without a revision there is no deduplication key, so a handback here would
    // re-prompt the worker on every pass.
    let mut entry = entry();
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "", "base-1", true, 2),
        RecoveryPlan::Idle
    );
    assert_eq!(entry.merge_recovery.charged, 0);
}

#[test]
fn a_disabled_recovery_never_reaches_a_worker() {
    // `plan` is what selects the delivery path, and with recovery off no reason
    // and no revision may select it: the switch still decides whether a worker is
    // ever driven, and only `Dispatch` resolves a session or sends a prompt.
    let config = crate::overseer::config::OverseerConfig::default();
    assert!(!config.merge_recovery_enabled);
    let mut entry = entry();
    for reason in [
        "merge_state:dirty",
        "checks_not_green",
        "unprotected:unknown_remote",
    ] {
        for head in ["sha-1", "sha-2", ""] {
            for base in ["base-1", "base-2", ""] {
                assert!(matches!(
                    plan(
                        &mut entry,
                        reason,
                        head,
                        base,
                        false,
                        config.max_merge_recoveries
                    ),
                    RecoveryPlan::Idle | RecoveryPlan::Dropped
                ));
            }
        }
    }
    assert_eq!(entry.phase, LedgerPhase::PrOpened);
    assert_eq!(entry.merge_recovery.charged, 0);
}

#[test]
fn every_recorded_plan_reason_names_the_merge_recovery_step() {
    assert_eq!(
        disabled("merge_state:dirty"),
        "merge_recovery_disabled:merge_state:dirty"
    );
    assert_eq!(CAP_REACHED, "merge_recovery_cap_reached");
}

#[cfg(test)]
#[path = "merge_recovery_plan_classify_tests.rs"]
mod classify_tests;
