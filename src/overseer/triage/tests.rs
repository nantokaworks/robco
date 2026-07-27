use super::*;
use crate::Result;
use crate::overseer::{
    ledger::{Ledger, LedgerPhase},
    monitor::{Action, FailureOrigin},
    session::executable_script,
    triage::{
        queue::test_queue,
        result::{ParseError, parse},
    },
};
use chrono::Utc;
use std::{thread, time::Instant};

pub(super) fn case() -> ExceptionCase {
    ExceptionCase {
        id: "case-1".into(),
        kind: "worker_failed".into(),
        task_id: "task-1".into(),
        display_id: "#1".into(),
        worker_id: "worker-1".into(),
        repo: "/repo".into(),
        reason: "stuck".into(),
        task_state: "in_progress".into(),
    }
}

fn ledger() -> Ledger {
    Ledger {
        entries: vec![LedgerEntry {
            task_id: "task-1".into(),
            display_id: "#1".into(),
            repo: "/repo".into(),
            agent_id: "worker-1".into(),
            branch: "task-1".into(),
            phase: LedgerPhase::Failed,
            dispatched_at: Utc::now(),
            settled_at: None,
            retries: 0,
            pr_url: None,
            branch_updates: 0,
            merge_recovery: Default::default(),
            merge_hold: Default::default(),
            manual_merge_skip: None,
        }],
        ..Ledger::default()
    }
}

fn no_scribble(_: &str, _: &str) -> Result<()> {
    Ok(())
}

#[test]
fn skip_result_adds_task_to_skip_list() {
    let temp = tempfile::tempdir().unwrap();
    let mut ledger = ledger();
    apply_session_result_with(
        SessionResult::Result(br#"{"outcome":"skip","reason":"not actionable"}"#.to_vec()),
        &mut ledger,
        &case(),
        &temp.path().join("decisions.jsonl"),
        &no_scribble,
    )
    .unwrap();
    assert_eq!(ledger.skip_list, ["task-1"]);
}

#[test]
fn timeout_escalates() {
    let temp = tempfile::tempdir().unwrap();
    let script = executable_script(temp.path(), "sleep 5");
    let profile = Profile {
        name: "test".into(),
        program: script.to_string_lossy().into_owned(),
        autonomous_args: Vec::new(),
        model: None,
        backend: None,
    };
    let result = EphemeralSession {
        profile: &profile,
        case_dir: temp.path(),
        timeout: Duration::from_millis(75),
        env: &Default::default(),
    }
    .run(&result::is_complete);
    assert_eq!(result, SessionResult::TimedOut);
    let mut ledger = ledger();
    apply_session_result_with(
        result,
        &mut ledger,
        &case(),
        &temp.path().join("decisions.jsonl"),
        &no_scribble,
    )
    .unwrap();
    assert_eq!(ledger.entries[0].phase, LedgerPhase::Escalated);
}

/// Triage escalates outside the reconcile pass, and the next pass sees an entry
/// that is already terminal and leaves it alone — so if triage does not stamp
/// the entry here, nothing ever will and the history cannot order it.
#[test]
fn malformed_result_escalates_and_records_when_the_entry_settled() {
    let temp = tempfile::tempdir().unwrap();
    let mut ledger = ledger();
    let escalate = |ledger: &mut Ledger| {
        apply_session_result_with(
            SessionResult::Result(b"not-json".to_vec()),
            ledger,
            &case(),
            &temp.path().join("decisions.jsonl"),
            &no_scribble,
        )
        .unwrap();
    };

    escalate(&mut ledger);
    assert_eq!(ledger.entries[0].phase, LedgerPhase::Escalated);
    let settled = ledger.entries[0].settled_at;
    assert!(settled.is_some());

    // A repeat escalation of the same entry must not move the timestamp.
    escalate(&mut ledger);
    assert_eq!(ledger.entries[0].settled_at, settled);
}

#[test]
fn unknown_action_is_rejected_and_logged() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("decisions.jsonl");
    let mut ledger = ledger();
    let raw = br#"{
        "outcome":"resolved",
        "action":{"name":"run_shell","command":"rm -rf /"},
        "reason":"try command"
    }"#;
    apply_session_result_with(
        SessionResult::Result(raw.to_vec()),
        &mut ledger,
        &case(),
        &log_path,
        &no_scribble,
    )
    .unwrap();
    let log = fs::read_to_string(log_path).unwrap();
    assert!(log.contains("rejected triage action"));
    assert_eq!(ledger.entries[0].phase, LedgerPhase::Escalated);
}

#[test]
fn partial_result_write_is_polled_until_json_is_complete() {
    let temp = tempfile::tempdir().unwrap();
    let script = executable_script(
        temp.path(),
        "printf '{}' > result.json\nsleep 0.1\nprintf '{\"outcome\":\"resolved\",\"reason\":\"done\"}' > result.json",
    );
    let profile = Profile {
        name: "test".into(),
        program: script.to_string_lossy().into_owned(),
        autonomous_args: Vec::new(),
        model: None,
        backend: None,
    };
    let result = EphemeralSession {
        profile: &profile,
        case_dir: temp.path(),
        timeout: Duration::from_secs(2),
        env: &Default::default(),
    }
    .run(&result::is_complete);
    let SessionResult::Result(raw) = result else {
        panic!("expected complete result, got {result:?}");
    };
    assert!(serde_json::from_slice::<serde_json::Value>(&raw).is_ok());
}

#[test]
fn pending_queue_is_reconstructed_after_restart() {
    let temp = tempfile::tempdir().unwrap();
    let mut queue = test_queue(temp.path()).unwrap();
    queue
        .enqueue(
            &[Action::MarkFailed {
                task_id: "task-1".into(),
                reason: "dead".into(),
                origin: FailureOrigin::Worker,
            }],
            &ledger(),
            &Observations::default(),
        )
        .unwrap();
    assert_eq!(queue.pending_len(), 1);
    drop(queue);
    assert_eq!(test_queue(temp.path()).unwrap().pending_len(), 1);
}

#[test]
fn queue_tick_starts_session_without_blocking_daemon() {
    let temp = tempfile::tempdir().unwrap();
    let script = executable_script(
        temp.path(),
        "touch ../../started\nwhile [ ! -f ../../release ]; do sleep 0.01; done\nprintf '{\"outcome\":\"skip\",\"reason\":\"done\"}' > result.json",
    );
    let mut config = Config {
        profiles: vec![Profile {
            name: "test".into(),
            program: script.to_string_lossy().into_owned(),
            autonomous_args: Vec::new(),
            model: None,
            backend: None,
        }],
        ..Default::default()
    };
    config.overseer.triage_profile = Some("test".into());
    config.overseer.triage_timeout_mins = 1;
    let mut queue = test_queue(temp.path()).unwrap();
    queue
        .enqueue(
            &[Action::MarkFailed {
                task_id: "task-1".into(),
                reason: "dead".into(),
                origin: FailureOrigin::Worker,
            }],
            &ledger(),
            &Observations::default(),
        )
        .unwrap();
    let mut ledger = ledger();
    let tick_started = Instant::now();
    queue.tick(&config, &mut ledger).unwrap();
    assert!(
        tick_started.elapsed() < Duration::from_secs(5),
        "queue tick blocked the daemon"
    );
    assert!(queue.is_active());
    assert_eq!(queue.pending_len(), 1);

    let started = temp.path().join("started");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !started.exists() {
        assert!(Instant::now() < deadline, "triage session did not start");
        thread::sleep(Duration::from_millis(10));
    }
    assert!(queue.is_active());

    fs::write(temp.path().join("release"), []).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while queue.is_active() {
        assert!(Instant::now() < deadline, "triage session did not complete");
        queue.tick(&config, &mut ledger).unwrap();
        if queue.is_active() {
            thread::sleep(Duration::from_millis(25));
        }
    }
    queue.acknowledge_completion().unwrap();
    assert_eq!(queue.pending_len(), 0);
    assert_eq!(ledger.skip_list, ["task-1"]);
}

#[test]
fn live_worker_prevents_task_lock_release() {
    let raw = br#"{
        "outcome":"resolved",
        "action":{"name":"dropr_task_status_update","task_id":"task-1","status":"ready"},
        "reason":"release"
    }"#;
    let rejected = parse(raw, "task-1", "worker-1", &|_| true);
    assert!(
        matches!(rejected, Err(ParseError::RejectedAction(message)) if message.contains("alive"))
    );
    assert!(parse(raw, "task-1", "worker-1", &|_| false).is_ok());
}
