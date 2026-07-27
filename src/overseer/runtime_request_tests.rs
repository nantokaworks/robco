use super::*;
use crate::overseer::ledger::{LedgerEntry, LedgerPhase};

#[test]
fn enqueue_then_drain_applies_and_acks() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("requests");
    let mut ledger = Ledger::default();
    ledger.counters.consecutive_failures = 99;
    let mut config = Config::default();
    config.overseer.dispatch_enabled = false;
    enqueue_in(
        &dir,
        RuntimeRequest::ResetCircuit {
            source: "test".into(),
            at: Utc::now(),
        },
    )
    .unwrap();

    assert!(drain_in(&dir, &mut ledger, &mut config).unwrap());
    assert_eq!(ledger.counters.consecutive_failures, 0);
    assert!(config.overseer.dispatch_enabled);
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
    assert!(!drain_in(&dir, &mut ledger, &mut config).unwrap());
    assert_eq!(ledger.counters.consecutive_failures, 0);
}

#[test]
fn panic_escalate_marks_workers_including_pr_opened() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("requests");
    let mut ledger = Ledger {
        entries: vec![ledger_entry("worker-1", LedgerPhase::PrOpened)],
        ..Ledger::default()
    };
    let mut config = Config::default();
    enqueue_in(
        &dir,
        RuntimeRequest::PanicEscalate {
            source: "test".into(),
            agent_ids: vec!["worker-1".into()],
            at: Utc::now(),
        },
    )
    .unwrap();

    assert!(!drain_in(&dir, &mut ledger, &mut config).unwrap());
    assert_eq!(ledger.entries[0].phase, LedgerPhase::Escalated);
}

#[test]
fn merge_completed_wakes_the_daemon_without_touching_state() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("requests");
    let mut ledger = Ledger {
        entries: vec![ledger_entry("worker-1", LedgerPhase::PrOpened)],
        ..Ledger::default()
    };
    ledger.counters.consecutive_failures = 2;
    let mut config = Config::default();
    enqueue_in(
        &dir,
        RuntimeRequest::MergeCompleted {
            source: "ui".into(),
            repo: "/repo".into(),
            at: Utc::now(),
        },
    )
    .unwrap();

    assert!(!drain_in(&dir, &mut ledger, &mut config).unwrap());
    // The merge moves the ledger only once the daemon has observed it, so the
    // request itself leaves the phase and the failure streak alone.
    assert_eq!(ledger.entries[0].phase, LedgerPhase::PrOpened);
    assert_eq!(ledger.counters.consecutive_failures, 2);
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
}

#[test]
fn drain_missing_dir_is_noop() {
    let temp = tempfile::tempdir().unwrap();
    let mut ledger = Ledger::default();
    let mut config = Config::default();

    assert!(!drain_in(&temp.path().join("missing"), &mut ledger, &mut config).unwrap());
}

#[test]
fn corrupt_file_is_skipped() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("requests");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("bad.json"), "not json").unwrap();
    let mut ledger = Ledger::default();
    let mut config = Config::default();

    assert!(!drain_in(&dir, &mut ledger, &mut config).unwrap());
    assert!(!dir.join("bad.json").exists());
    assert!(dir.join("bad.json.corrupt").exists());
}

#[test]
fn drain_applies_and_acks_all_pending_requests() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("requests");
    let mut ledger = Ledger {
        entries: vec![ledger_entry("worker-1", LedgerPhase::PrOpened)],
        ..Ledger::default()
    };
    ledger.counters.consecutive_failures = 7;
    let mut config = Config::default();
    config.overseer.dispatch_enabled = false;
    enqueue_in(
        &dir,
        RuntimeRequest::PanicEscalate {
            source: "test".into(),
            agent_ids: vec!["worker-1".into()],
            at: Utc::now(),
        },
    )
    .unwrap();
    enqueue_in(
        &dir,
        RuntimeRequest::ResetCircuit {
            source: "test".into(),
            at: Utc::now(),
        },
    )
    .unwrap();

    assert!(drain_in(&dir, &mut ledger, &mut config).unwrap());
    assert_eq!(ledger.counters.consecutive_failures, 0);
    assert!(config.overseer.dispatch_enabled);
    assert_eq!(ledger.entries[0].phase, LedgerPhase::Escalated);
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
}

fn ledger_entry(agent_id: &str, phase: LedgerPhase) -> LedgerEntry {
    LedgerEntry {
        task_id: format!("task-{agent_id}"),
        display_id: "#202".into(),
        repo: "nantokaworks/robco".into(),
        agent_id: agent_id.into(),
        branch: "task-202".into(),
        phase,
        dispatched_at: Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: Some("https://example.test/pr/202".into()),
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        manual_merge_skip: None,
        merge_judge_fail_safes: 0,
        merge_hold_cap_escalated: false,
        merge_hold_rechecks: 0,
    }
}
