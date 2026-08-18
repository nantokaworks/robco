use chrono::Utc;

use super::*;
use crate::overseer::ledger::LedgerPhase;

fn entry(task_id: &str, display_id: &str, phase: LedgerPhase) -> LedgerEntry {
    LedgerEntry {
        task_id: task_id.into(),
        display_id: display_id.into(),
        repo: "/repo".into(),
        agent_id: "agent-1".into(),
        branch: "branch".into(),
        phase,
        dispatched_at: Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: Some("https://github.com/acme/widgets/pull/1".into()),
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
fn finds_by_task_id_or_display_id() {
    let ledger = Ledger {
        entries: vec![entry("task-1", "#1", LedgerPhase::Escalated)],
        ..Default::default()
    };
    assert_eq!(
        find_ledger_entry(&ledger, "task-1").unwrap().task_id,
        "task-1"
    );
    assert_eq!(find_ledger_entry(&ledger, "#1").unwrap().task_id, "task-1");
    assert!(find_ledger_entry(&ledger, "missing").is_err());
}

#[test]
fn queue_approval_sends_an_approve_request_naming_the_task_and_user() {
    let (tx, rx) = std::sync::mpsc::channel();
    queue_approval(&tx, "task-1", "user-1").unwrap();
    assert_eq!(
        rx.try_recv().unwrap(),
        LedgerRequest::Approve {
            task: "task-1".into(),
            user_id: "user-1".into(),
        }
    );
}

#[test]
fn queue_approval_errs_when_the_daemon_channel_is_closed() {
    let (tx, rx) = std::sync::mpsc::channel();
    drop(rx);
    assert!(queue_approval(&tx, "task-1", "user-1").is_err());
}
