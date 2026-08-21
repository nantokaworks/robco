use super::*;
use crate::overseer::discord::{
    commands::Command,
    ops_state::{DeliveryOutcome, MAX_DELIVERY_ATTEMPTS},
};
use std::{fs, path::Path};

#[test]
fn unrelated_answer_executes_without_resolving_case() {
    let temp = tempfile::tempdir().unwrap();
    let mut ops = mapped_agent(temp.path());
    let answer = Command::Answer {
        agent: "other-worker".into(),
        text: "proceed".into(),
    };
    assert!(
        ops.resolve_answer("20", Some("case-1"), &answer)
            .unwrap()
            .is_empty()
    );
    assert!(
        ops.discover()
            .unwrap()
            .iter()
            .all(|effect| { !matches!(effect, Effect::DeliverResolution { .. }) })
    );
}

#[test]
fn resolution_delivery_retries_then_exhausts() {
    let temp = tempfile::tempdir().unwrap();
    let mut ops = mapped_agent(temp.path());
    let answer = Command::Answer {
        agent: "worker-1".into(),
        text: "proceed".into(),
    };
    ops.resolve_answer("20", Some("case-1"), &answer).unwrap();
    for _ in 1..MAX_DELIVERY_ATTEMPTS {
        assert_eq!(
            ops.resolution_delivery("case-1", false, false).unwrap(),
            DeliveryOutcome::Retry
        );
        assert!(
            ops.discover()
                .unwrap()
                .iter()
                .any(|effect| { matches!(effect, Effect::DeliverResolution { .. }) })
        );
    }
    assert_eq!(
        ops.resolution_delivery("case-1", false, false).unwrap(),
        DeliveryOutcome::Exhausted
    );
    assert!(
        ops.discover()
            .unwrap()
            .iter()
            .any(|effect| { matches!(effect, Effect::DeliverResolution { .. }) })
    );
    ops.finalize_exhausted_resolution("case-1").unwrap();
    assert!(
        ops.discover()
            .unwrap()
            .iter()
            .all(|effect| { !matches!(effect, Effect::DeliverResolution { .. }) })
    );
}

#[test]
fn persisted_intent_reuses_reconciled_thread_after_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let case = sample_case();
    write_case(temp.path(), &case);
    let state = temp.path().join("ops/threads.json");
    let mut first = load(temp.path(), state.clone());
    assert!(matches!(
        first.discover().unwrap().as_slice(),
        [Effect::OpenThread(_)]
    ));
    drop(first);
    let mut recovered = load(temp.path(), state.clone());
    assert!(matches!(
        recovered.discover().unwrap().as_slice(),
        [Effect::OpenThread(_)]
    ));
    recovered.thread_opened(case, "20".into()).unwrap();
    let mut completed = load(temp.path(), state);
    assert!(
        completed
            .discover()
            .unwrap()
            .iter()
            .all(|effect| { !matches!(effect, Effect::OpenThread(_)) })
    );
}

#[test]
fn failed_opening_delivery_is_retried_after_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let case = sample_case();
    write_case(temp.path(), &case);
    let state = temp.path().join("ops/threads.json");
    let mut first = load(temp.path(), state.clone());
    first.discover().unwrap();
    first.thread_opened(case, "20".into()).unwrap();
    drop(first);
    let mut recovered = load(temp.path(), state);
    assert!(matches!(
        recovered.discover().unwrap().as_slice(),
        [Effect::Opening { case_id, .. }] if case_id == "case-1"
    ));
    recovered.opening_delivered("case-1").unwrap();
    assert!(recovered.discover().unwrap().is_empty());
}

fn mapped_agent(root: &Path) -> OpsAgent {
    let case = sample_case();
    write_case(root, &case);
    let mut ops = load(root, root.join("ops/threads.json"));
    ops.discover().unwrap();
    ops.thread_opened(case, "20".into()).unwrap();
    ops.opening_delivered("case-1").unwrap();
    ops
}

fn load(root: &Path, state: std::path::PathBuf) -> OpsAgent {
    OpsAgent::load("10".into(), vec!["user".into()], root.join("triage"), state).unwrap()
}

fn write_case(root: &Path, case: &ExceptionCase) {
    let dir = root.join("triage").join(&case.id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("case.json"), serde_json::to_vec(case).unwrap()).unwrap();
    fs::write(dir.join("outcome.json"), br#"{"outcome":"escalate"}"#).unwrap();
}

fn sample_case() -> ExceptionCase {
    ExceptionCase {
        id: "case-1".into(),
        kind: "blocked".into(),
        task_id: "task-1".into(),
        dropr_task_id: None,
        display_id: "#1".into(),
        worker_id: "worker-1".into(),
        repo: "org/repo".into(),
        reason: "question".into(),
        task_state: "open".into(),
    }
}
