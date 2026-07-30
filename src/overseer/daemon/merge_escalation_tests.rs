use super::*;
use crate::overseer::ledger::{LedgerEntry, LedgerPhase, MergeHold};
use chrono::Duration;

fn now() -> DateTime<Utc> {
    "2026-07-30T00:00:00Z".parse().unwrap()
}

fn entry() -> LedgerEntry {
    LedgerEntry {
        task_id: "task".into(),
        display_id: "#1".into(),
        repo: "/repo".into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
        phase: LedgerPhase::Escalated,
        dispatched_at: now(),
        settled_at: None,
        retries: 0,
        pr_url: Some("https://pr/1".into()),
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: MergeHold {
            reason: Some("merge_state:dirty".into()),
            head: Some("sha-1".into()),
            passes: 30,
            escalated: true,
        },
        manual_merge_skip: None,
        merge_judge_fail_safes: 0,
        merge_hold_cap_escalated: true,
        merge_hold_rechecks: 0,
        merge_hold_recheck_reason: Some("merge_state:dirty".into()),
        merge_hold_recheck_head: Some("sha-1".into()),
        merge_hold_stuck_notified: false,
    }
}

#[test]
fn terminal_reasons_are_named_by_the_module() {
    for reason in [
        merge_recovery::CAP_REACHED,
        merge_judge_fail_safe::CAP_REACHED,
        super::super::merge_decision::CLOSED_UNMERGED,
        "merge_hold_recheck_exhausted:merge_state:dirty",
        "merge_recovery_skipped:missing_session:agent-1",
        "merge_hold_stuck:merge_state:dirty",
    ] {
        assert!(is_terminal(reason), "{reason} should be terminal");
        assert!(!is_transient(reason), "{reason} should not be transient");
    }
}

#[test]
fn the_hold_cap_reason_is_transient() {
    let reason = "merge_hold_cap_reached:merge_state:dirty";
    assert!(is_transient(reason));
    assert!(!is_terminal(reason));
}

#[test]
fn reasons_outside_the_vocabulary_are_neither() {
    for reason in [
        "judge_veto:no rollback",
        "judge_escalate:risky",
        "autonomy_envelope",
        "behind_update_cap_reached",
        "worker blocked",
        "task_escalated",
    ] {
        assert!(!is_terminal(reason), "{reason} should not be terminal");
        assert!(!is_transient(reason), "{reason} should not be transient");
        assert_eq!(notify(DecisionKind::Escalate, reason), None);
    }
}

#[test]
fn notify_only_tags_escalate_decisions() {
    assert_eq!(
        notify(DecisionKind::Hold, merge_recovery::CAP_REACHED),
        None
    );
    assert_eq!(
        notify(DecisionKind::Escalate, merge_recovery::CAP_REACHED),
        Some(true)
    );
}

/// The scenario this module exists for: a purely static conflict spends no
/// recheck budget at all (see `merge_hold_recheck`'s module doc), so nothing
/// else ever logs a second decision about it. The sweep is the only thing
/// standing between that and silence forever.
#[test]
fn a_static_conflict_notifies_once_after_the_stuck_threshold_and_then_stays_quiet() {
    let mut ledger = Ledger {
        entries: vec![entry()],
        ..Default::default()
    };

    // Freshly escalated: `settled_at` is stamped for the first time, and the
    // age is zero, so nothing fires yet.
    sweep_stuck(&mut ledger, now(), 10).unwrap();
    assert_eq!(ledger.entries[0].settled_at, Some(now()));
    assert!(!ledger.entries[0].merge_hold_stuck_notified);

    // Short of the threshold: still quiet.
    let almost = now() + STUCK_AFTER - Duration::minutes(1);
    sweep_stuck(&mut ledger, almost, 10).unwrap();
    assert!(!ledger.entries[0].merge_hold_stuck_notified);

    // Past the threshold: notifies exactly once.
    let stuck_at = now() + STUCK_AFTER + Duration::minutes(1);
    sweep_stuck(&mut ledger, stuck_at, 10).unwrap();
    assert!(ledger.entries[0].merge_hold_stuck_notified);

    // A later pass, still stuck, does not repeat.
    sweep_stuck(&mut ledger, stuck_at + Duration::hours(1), 10).unwrap();
}

#[test]
fn an_entry_no_longer_eligible_for_reconsideration_is_left_to_its_own_terminal_decision() {
    let mut ledger = Ledger {
        entries: vec![LedgerEntry {
            // The recheck budget is already spent — `merge_hold_recheck`
            // already logged its own terminal decision for this entry, so
            // the sweep must not pile a second one on top.
            merge_hold_rechecks: 10,
            settled_at: Some(now() - (STUCK_AFTER * 10)),
            ..entry()
        }],
        ..Default::default()
    };
    sweep_stuck(&mut ledger, now(), 10).unwrap();
    assert!(!ledger.entries[0].merge_hold_stuck_notified);
}

#[test]
fn a_non_escalated_entry_is_never_swept() {
    let mut ledger = Ledger {
        entries: vec![LedgerEntry {
            phase: LedgerPhase::PrOpened,
            settled_at: Some(now() - (STUCK_AFTER * 10)),
            ..entry()
        }],
        ..Default::default()
    };
    sweep_stuck(&mut ledger, now(), 10).unwrap();
    assert!(!ledger.entries[0].merge_hold_stuck_notified);
    assert!(ledger.entries[0].settled_at.is_some());
}
