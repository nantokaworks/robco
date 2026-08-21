use super::*;
use crate::overseer::ledger::LedgerPhase;

fn entry(task_id: &str, display_id: &str, pr_url: Option<&str>) -> LedgerEntry {
    LedgerEntry {
        task_id: task_id.into(),
        dropr_task_id: None,
        display_id: display_id.into(),
        repo: "/no/such/repo".into(),
        agent_id: "agent-1".into(),
        branch: "branch".into(),
        phase: LedgerPhase::PrOpened,
        dispatched_at: Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: pr_url.map(str::to_owned),
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
fn requests_only_mutate_daemon_owned_ledger() {
    let mut ledger = Ledger::default();
    apply(
        &mut ledger,
        LedgerRequest::Skip {
            task: "task-1".into(),
            user_id: "user-1".into(),
        },
    )
    .unwrap();
    assert_eq!(ledger.skip_list, ["task-1"]);
    assert!(
        apply(
            &mut ledger,
            LedgerRequest::Retry {
                task: "missing".into(),
                user_id: "user-1".into(),
            }
        )
        .is_err()
    );
}

#[test]
fn approve_refuses_a_task_the_ledger_does_not_know() {
    let mut ledger = Ledger::default();
    assert!(
        apply(
            &mut ledger,
            LedgerRequest::Approve {
                task: "missing".into(),
                user_id: "user-1".into(),
            }
        )
        .is_err()
    );
}

#[test]
fn approve_refuses_an_entry_with_no_pull_request_recorded() {
    let mut ledger = Ledger {
        entries: vec![entry("task-1", "#1", None)],
        ..Default::default()
    };
    assert!(
        apply(
            &mut ledger,
            LedgerRequest::Approve {
                task: "#1".into(),
                user_id: "user-1".into(),
            }
        )
        .is_err()
    );
    assert!(ledger.entries[0].merge_approval.is_none());
}

#[test]
fn approve_resets_an_exhausted_recheck_budget() {
    let mut ledger_entry = entry(
        "task-1",
        "#1",
        Some("https://github.com/acme/widgets/pull/1"),
    );
    ledger_entry.phase = LedgerPhase::Escalated;
    ledger_entry.merge_hold_cap_escalated = true;
    // A budget already spent — exactly the entry `merge_repo_pass::run`'s
    // `reconsidering` check would otherwise never look at again.
    ledger_entry.merge_hold_rechecks = 10;
    record_approval(
        &mut ledger_entry,
        "/repo".into(),
        Some("https://github.com/acme/widgets/pull/1".into()),
        "user-1".into(),
        "deadbeef".into(),
        "discord",
    )
    .unwrap();
    assert_eq!(
        ledger_entry.merge_approval.as_ref().unwrap().head,
        "deadbeef"
    );
    assert_eq!(ledger_entry.merge_hold_rechecks, 0);
    assert!(ledger_entry.merge_hold_cap_escalated);
}

#[test]
fn approve_matches_by_display_id_but_leaves_no_approval_when_the_pull_request_cannot_be_read() {
    let mut ledger = Ledger {
        entries: vec![entry(
            "task-1",
            "#1",
            Some("https://github.com/acme/widgets/pull/1"),
        )],
        ..Default::default()
    };
    assert!(
        apply(
            &mut ledger,
            LedgerRequest::Approve {
                task: "#1".into(),
                user_id: "user-1".into(),
            }
        )
        .is_err()
    );
    assert!(ledger.entries[0].merge_approval.is_none());
}

#[test]
fn run_is_a_no_op_through_apply_and_never_touches_the_ledger() {
    let mut ledger = Ledger::default();
    apply(
        &mut ledger,
        LedgerRequest::Run {
            task: "task-1".into(),
            user_id: "user-1".into(),
        },
    )
    .unwrap();
    assert_eq!(ledger, Ledger::default());
}
