use super::*;
use crate::overseer::dispatch::GateDecision;

fn candidate(id: &str) -> Candidate {
    Candidate {
        task_id: id.into(),
        display_id: format!("#{id}"),
        title: id.into(),
        repo: format!("/{id}"),
        author: "allowed".into(),
    }
}

#[test]
fn circuit_stops_mid_plan() {
    let decisions = [candidate("1"), candidate("2")]
        .into_iter()
        .map(|candidate| GateDecision {
            candidate: Some(candidate),
            dispatch: true,
            reason: "ready".into(),
        })
        .collect();
    let mut failures = 0;
    let mut spawned = Vec::new();
    let opened = execute_plan(
        decisions,
        1,
        &mut failures,
        |candidate| {
            spawned.push(candidate.task_id.clone());
            Err(std::io::Error::other("spawn failed").into())
        },
        |_, _, _| Ok(()),
    )
    .unwrap();
    assert!(opened);
    assert_eq!(spawned, ["1"]);
}

#[test]
fn repo_skip_emits_skip_decision() {
    let mut captured = None;
    log_repo_skip("/repo", "repo_path_missing", |entry| {
        captured = Some(entry.clone());
        Ok(())
    })
    .unwrap();
    let entry = captured.unwrap();
    assert_eq!(entry.kind, DecisionKind::Skip);
    assert_eq!(entry.reason, "repo_path_missing");
    assert_eq!(entry.repo.as_deref(), Some("/repo"));
}

#[test]
fn fetch_failure_emits_skip_decision() {
    let mut captured = None;
    log_ready_failure(
        "/repo",
        "workspace-1",
        dropr::ReadyDispatchError::Parse,
        |entry| {
            captured = Some(entry.clone());
            Ok(())
        },
    )
    .unwrap();
    let entry = captured.unwrap();
    assert_eq!(entry.kind, DecisionKind::Skip);
    assert_eq!(entry.reason, "ready_parse_failed:workspace-1");
    assert_eq!(entry.repo.as_deref(), Some("/repo"));
}
