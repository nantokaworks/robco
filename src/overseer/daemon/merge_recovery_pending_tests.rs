use super::*;
use crate::overseer::ledger::LedgerPhase;

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

#[test]
fn a_first_hold_records_one_attempt() {
    let mut entry = entry();
    let attempts = hold(&mut entry, "checks_not_green", "sha-1", "base-1");
    assert_eq!(attempts, 1);
    let pending = entry.merge_recovery.pending.as_ref().unwrap();
    assert_eq!(pending.reason, "checks_not_green");
    assert_eq!(pending.head, "sha-1");
    assert_eq!(pending.base, "base-1");
    assert_eq!(pending.attempts, 1);
}

/// The same session staying busy across passes for the identical failure is
/// a continuation, not a new instruction: the attempt count accumulates
/// toward the retry bound.
#[test]
fn a_repeated_hold_for_the_same_instruction_accumulates_attempts() {
    let mut entry = entry();
    hold(&mut entry, "checks_not_green", "sha-1", "base-1");
    let attempts = hold(&mut entry, "checks_not_green", "sha-1", "base-1");
    assert_eq!(attempts, 2);
    assert_eq!(entry.merge_recovery.pending.as_ref().unwrap().attempts, 2);
}

/// A second hold reason for the same entry replaces the pending one instead
/// of queueing behind it — the worker never receives two.
#[test]
fn a_newer_reason_replaces_the_pending_one_and_resets_attempts() {
    let mut entry = entry();
    hold(&mut entry, "checks_not_green", "sha-1", "base-1");
    hold(&mut entry, "checks_not_green", "sha-1", "base-1");
    assert_eq!(entry.merge_recovery.pending.as_ref().unwrap().attempts, 2);

    let attempts = hold(&mut entry, "merge_state:dirty", "sha-1", "base-1");
    assert_eq!(attempts, 1);
    let pending = entry.merge_recovery.pending.as_ref().unwrap();
    assert_eq!(pending.reason, "merge_state:dirty");
    assert_eq!(pending.attempts, 1);
}

/// A worker that pushed a fix presents a new head: the old handback no
/// longer describes the current revision, so it starts its own count too.
#[test]
fn a_new_head_starts_its_own_attempt_count() {
    let mut entry = entry();
    hold(&mut entry, "checks_not_green", "sha-1", "base-1");
    hold(&mut entry, "checks_not_green", "sha-1", "base-1");

    let attempts = hold(&mut entry, "checks_not_green", "sha-2", "base-1");
    assert_eq!(attempts, 1);
}

#[test]
fn discard_clears_a_pending_handback() {
    let mut entry = entry();
    hold(&mut entry, "checks_not_green", "sha-1", "base-1");
    assert!(entry.merge_recovery.pending.is_some());

    discard(&mut entry);
    assert!(entry.merge_recovery.pending.is_none());
}

#[test]
fn discard_is_a_no_op_when_nothing_is_pending() {
    let mut entry = entry();
    discard(&mut entry);
    assert!(entry.merge_recovery.pending.is_none());
}
