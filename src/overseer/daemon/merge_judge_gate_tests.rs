use super::*;
use crate::overseer::ledger::{LedgerPhase, OperatorOverride};

fn base_entry(phase: LedgerPhase) -> LedgerEntry {
    LedgerEntry {
        task_id: "task".into(),
        display_id: "#1".into(),
        repo: "/repo".into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
        phase,
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
        prerequisite_wait: None,
        merge_hold_stuck_notified: false,
        worker_escalated: false,
        operator_override: None,
    }
}

#[test]
fn veto_escalates_and_cannot_be_selected_again_at_same_revision() {
    let mut entry = base_entry(LedgerPhase::PrOpened);
    assert!(!judgment_allows_merge(&mut entry, MergeJudgment::Veto));
    assert_eq!(entry.phase, LedgerPhase::Escalated);
    assert_ne!(entry.phase, LedgerPhase::PrOpened);
}

#[test]
fn take_operator_override_returns_false_and_leaves_the_entry_alone_when_none_is_pending() {
    let mut entry = base_entry(LedgerPhase::Escalated);
    assert!(!take_operator_override(&mut entry, "head1", "judge_veto:x").unwrap());
    assert!(entry.operator_override.is_none());
}

#[test]
fn take_operator_override_consumes_but_refuses_a_mismatched_head() {
    let mut entry = base_entry(LedgerPhase::Escalated);
    entry.operator_override = Some(OperatorOverride {
        head: "old-head".into(),
        granted_at: chrono::Utc::now(),
    });

    assert!(!take_operator_override(&mut entry, "new-head", "judge_veto:x").unwrap());

    // Taken either way: a stale grant is spent, not retried against a
    // revision it was never approved for.
    assert!(entry.operator_override.is_none());
}

#[test]
fn take_operator_override_bypasses_on_a_matching_head() {
    let mut entry = base_entry(LedgerPhase::Escalated);
    entry.operator_override = Some(OperatorOverride {
        head: "abc123".into(),
        granted_at: chrono::Utc::now(),
    });

    assert!(take_operator_override(&mut entry, "abc123", "judge_veto:nope").unwrap());

    assert!(entry.operator_override.is_none());
    // `take_operator_override` never touches `phase` itself — `judge_allows`
    // only calls `judgment_allows_merge` on the non-bypassed path, so a
    // bypassed entry stays whatever phase it already was instead of being
    // marked `Escalated` right before it merges.
    assert_eq!(entry.phase, LedgerPhase::Escalated);
}
