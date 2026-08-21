//! `EphemeralSession` timeout/polling behavior and `ExceptionQueue`
//! persistence — split out of `tests.rs` to keep that file under this
//! project's source file size limit. Shares `case`/`ledger`/`no_scribble`
//! from there rather than drifting its own copies.

use super::{
    apply_session_result_with,
    tests::{case, ledger, no_scribble},
};
use crate::config::{Config, Profile};
use crate::overseer::{
    ledger::LedgerPhase,
    monitor::{Action, FailureOrigin, Observations},
    session::{BRIEFING_PROMPT, EphemeralSession, SessionResult, executable_script},
    triage::{queue::test_queue, result},
};
use std::{
    fs, thread,
    time::{Duration, Instant},
};

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
        prompt: BRIEFING_PROMPT,
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
        prompt: BRIEFING_PROMPT,
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
