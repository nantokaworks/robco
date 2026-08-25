use super::*;
use crate::{
    Result,
    overseer::{
        ledger::Ledger,
        session::{SessionResult, executable_script},
        triage::{completion::replay_test, queue::test_queue, result::TriageAction},
    },
};
use std::{
    fs,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::Duration,
};

fn recovery_case() -> ExceptionCase {
    ExceptionCase {
        id: "recovery-case".into(),
        kind: "worker_failed".into(),
        task_id: "task-1".into(),
        dropr_task_id: None,
        display_id: "#1".into(),
        worker_id: "worker-1".into(),
        repo: "/repo".into(),
        reason: "failed".into(),
        task_state: "in_progress".into(),
    }
}

#[test]
fn corrupt_queue_starts_empty_and_records_decision() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("queue.json"), "").unwrap();
    let queue = test_queue(temp.path()).unwrap();
    assert_eq!(queue.pending_len(), 0);
    assert!(temp.path().join("queue.json.corrupt").exists());
    let log = fs::read_to_string(temp.path().join("decisions.jsonl")).unwrap();
    assert!(log.contains("triage queue unreadable; starting empty"));
}

#[test]
fn outcome_marker_prevents_action_reexecution() {
    let temp = tempfile::tempdir().unwrap();
    let case = recovery_case();
    let case_dir = temp.path().join("cases").join(&case.id);
    let log_path = temp.path().join("decisions.jsonl");
    let calls = AtomicUsize::new(0);
    let action = |_: &TriageAction, _: &ExceptionCase| -> Result<()> {
        assert!(case_dir.join("outcome.json").exists());
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    };
    let raw = br#"{"outcome":"skip","action":{"name":"robco_answer","agent_id":"worker-1","text":"ok"},"reason":"done"}"#;
    for _ in 0..2 {
        replay_test(
            SessionResult::Result(raw.to_vec()),
            &mut Ledger::default(),
            &case,
            &case_dir,
            &log_path,
            &action,
        )
        .unwrap();
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(case_dir.join("outcome.json").exists());
}

#[test]
fn dropping_session_handle_kills_worker_and_removes_pidfile() {
    let temp = tempfile::tempdir().unwrap();
    let script = executable_script(temp.path(), "while :; do :; done");
    let mut config = Config {
        profiles: vec![Profile {
            name: "test".into(),
            program: script.to_string_lossy().into_owned(),
            autonomous_args: Vec::new(),
            model: None,
            backend: None,
            clear_command: None,
        }],
        ..Default::default()
    };
    config.overseer.triage_profile = Some("test".into());
    config.overseer.triage_timeout_mins = 1;
    let root = temp.path().join("cases");
    let pid_path = root.join("recovery-case/session.pid");
    let handle = spawn_session(&config, &recovery_case(), &root);
    for _ in 0..100 {
        if pid_path.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(pid_path.exists());
    let pid = fs::read_to_string(&pid_path).unwrap();
    drop(handle);
    assert!(!pid_path.exists());
    #[cfg(unix)]
    assert!(
        !Command::new("kill")
            .args(["-0", pid.trim()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    );
}
