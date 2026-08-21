use super::*;

fn case(dropr_task_id: Option<&str>) -> ExceptionCase {
    ExceptionCase {
        id: "case-1".into(),
        kind: "worker_failed".into(),
        task_id: "task-1".into(),
        dropr_task_id: dropr_task_id.map(str::to_owned),
        display_id: "#1".into(),
        worker_id: "worker-1".into(),
        repo: "/repo".into(),
        reason: "stuck".into(),
        task_state: "in_progress".into(),
    }
}

/// The defect this guards: a case adopted from a live agent (not dispatched
/// through dropr) has no real dropr task id, so a triage action naming one
/// anyway must never reach dropr at all — see dropr:531, dropr:535.
#[test]
fn a_case_with_no_dropr_task_skips_the_write() {
    let mut called = false;
    let result = dropr_write(&case(None), || {
        called = true;
        Ok(())
    });
    assert!(result.is_ok());
    assert!(!called, "must not call dropr with no known dropr task");
}

#[test]
fn a_case_with_a_dropr_task_runs_the_write() {
    let mut called = false;
    let result = dropr_write(&case(Some("task-1")), || {
        called = true;
        Ok(())
    });
    assert!(result.is_ok());
    assert!(
        called,
        "existing behavior: a known dropr task still runs the write"
    );
}
