use super::*;
use crate::overseer::ledger::{LedgerPhase, MergeApproval, OperatorOverride};

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
    }
}

#[test]
fn take_operator_override_returns_false_and_leaves_the_entry_alone_when_none_is_pending() {
    let mut entry = base_entry(LedgerPhase::Escalated);
    assert!(!take_operator_override(&mut entry, "head1", "autonomy_envelope").unwrap());
    assert!(entry.operator_override.is_none());
}

#[test]
fn take_operator_override_consumes_but_refuses_a_mismatched_head() {
    let mut entry = base_entry(LedgerPhase::Escalated);
    entry.operator_override = Some(OperatorOverride {
        head: "old-head".into(),
        granted_at: chrono::Utc::now(),
    });

    assert!(!take_operator_override(&mut entry, "new-head", "autonomy_envelope").unwrap());

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

    assert!(take_operator_override(&mut entry, "abc123", "autonomy_envelope").unwrap());

    assert!(entry.operator_override.is_none());
    // `take_operator_override` never touches `phase` itself.
    assert_eq!(entry.phase, LedgerPhase::Escalated);
}

#[test]
fn take_merge_approval_returns_false_and_leaves_the_entry_alone_when_none_is_pending() {
    let mut entry = base_entry(LedgerPhase::PrOpened);
    assert!(!take_merge_approval(&mut entry, "head1").unwrap());
    assert!(entry.merge_approval.is_none());
}

#[test]
fn take_merge_approval_consumes_but_refuses_a_mismatched_head() {
    let mut entry = base_entry(LedgerPhase::PrOpened);
    entry.merge_approval = Some(MergeApproval {
        head: "old-head".into(),
        granted_at: chrono::Utc::now(),
    });

    assert!(!take_merge_approval(&mut entry, "new-head").unwrap());

    // Taken either way, and the drop itself is recorded (dropr:456): a
    // silently dropped approval would look like a merge that should
    // already have happened.
    assert!(entry.merge_approval.is_none());
}

#[test]
fn take_merge_approval_bypasses_on_a_matching_head() {
    let mut entry = base_entry(LedgerPhase::PrOpened);
    entry.merge_approval = Some(MergeApproval {
        head: "abc123".into(),
        granted_at: chrono::Utc::now(),
    });

    assert!(take_merge_approval(&mut entry, "abc123").unwrap());

    assert!(entry.merge_approval.is_none());
    assert_eq!(entry.phase, LedgerPhase::PrOpened);
}

#[test]
fn an_uncontended_envelope_allows_the_merge_without_touching_any_bypass() {
    let mut entry = base_entry(LedgerPhase::PrOpened);
    let config = Config::default();
    let value = serde_json::json!({
        "headRefOid": "abc123",
        "files": [{"path": "docs/readme.md"}],
        "changedFiles": 1,
        "additions": 1,
        "deletions": 0,
    });

    let judgment = merge_allows(&mut entry, &value, &config, 0).unwrap();

    assert!(matches!(judgment, Judgment::Allow));
    assert!(entry.operator_override.is_none());
}

#[test]
fn an_escalating_envelope_halts_without_a_pending_bypass() {
    let mut entry = base_entry(LedgerPhase::PrOpened);
    let config = Config::default();
    // Unknown facts (no file/line data at all) always escalate.
    let value = serde_json::json!({"headRefOid": "abc123"});

    let judgment = merge_allows(&mut entry, &value, &config, 0).unwrap();

    match judgment {
        Judgment::Halt(halt) => assert_eq!(halt.reason, "autonomy_envelope"),
        Judgment::Allow => panic!("expected a halt"),
    }
}

#[test]
fn an_escalating_envelope_is_bypassed_by_a_matching_operator_override() {
    let mut entry = base_entry(LedgerPhase::PrOpened);
    entry.operator_override = Some(OperatorOverride {
        head: "abc123".into(),
        granted_at: chrono::Utc::now(),
    });
    let config = Config::default();
    let value = serde_json::json!({"headRefOid": "abc123"});

    let judgment = merge_allows(&mut entry, &value, &config, 0).unwrap();

    assert!(matches!(judgment, Judgment::Allow));
    assert!(entry.operator_override.is_none());
}
