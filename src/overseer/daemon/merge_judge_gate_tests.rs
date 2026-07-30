use super::*;
use crate::overseer::ledger::LedgerPhase;

#[test]
fn veto_escalates_and_cannot_be_selected_again_at_same_revision() {
    let mut entry = LedgerEntry {
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
        prerequisite_wait: None,
        merge_hold_stuck_notified: false,
    };
    assert!(!judgment_allows_merge(&mut entry, MergeJudgment::Veto));
    assert_eq!(entry.phase, LedgerPhase::Escalated);
    assert_ne!(entry.phase, LedgerPhase::PrOpened);
}
