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
fn a_blocking_edge_holds_and_names_the_prerequisite() {
    let mut e = entry();
    let halt = apply(&mut e, Probe::Blocking("#7".into())).expect("held on the prerequisite");
    assert_eq!(halt.reason, "prerequisite_unmerged:#7");
    assert!(e.prerequisite_wait.is_some());
}

#[test]
fn a_blocking_edge_does_not_restart_an_already_running_wait() {
    let mut e = entry();
    let first = chrono::Utc::now() - chrono::Duration::hours(5);
    e.prerequisite_wait = Some(first);
    apply(&mut e, Probe::Blocking("#7".into()));
    assert_eq!(e.prerequisite_wait, Some(first));
}

#[test]
fn a_clear_probe_lets_the_gate_proceed_and_forgets_the_wait() {
    let mut e = entry();
    e.prerequisite_wait = Some(chrono::Utc::now());
    assert!(apply(&mut e, Probe::Clear).is_none());
    assert!(e.prerequisite_wait.is_none());
}

#[test]
fn a_failed_probe_holds_rather_than_letting_the_merge_proceed_unconfirmed() {
    let mut e = entry();
    let halt = apply(&mut e, Probe::Failed).expect("a failed probe must hold, not clear");
    assert_eq!(halt.reason, "prerequisite_probe_failed");
}

#[test]
fn a_failed_probe_does_not_disturb_an_existing_wait_timestamp() {
    let mut e = entry();
    let since = chrono::Utc::now() - chrono::Duration::hours(3);
    e.prerequisite_wait = Some(since);
    apply(&mut e, Probe::Failed);
    assert_eq!(e.prerequisite_wait, Some(since));
}
