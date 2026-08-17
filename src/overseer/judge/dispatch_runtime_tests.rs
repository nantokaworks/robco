use super::*;
use crate::overseer::dispatch::GateDecision;

fn candidate(id: &str) -> Candidate {
    Candidate {
        task_id: id.into(),
        display_id: format!("#{id}"),
        title: id.into(),
        repo: format!("/{id}"),
        author: "allowed".into(),
        priority: "medium".into(),
        workspace: "workspace-1".into(),
        priority_score: None,
        status: "open".into(),
        parent_task_id: None,
    }
}

fn decisions(candidates: &[&str]) -> Vec<GateDecision> {
    candidates
        .iter()
        .map(|id| GateDecision {
            candidate: Some(candidate(id)),
            dispatch: true,
            reason: "ready".into(),
        })
        .collect()
}

#[test]
fn a_repeatedly_failing_candidate_trips_its_own_circuit_without_stopping_others() {
    // The scoped fix for dropr:_ord_VtFSIiLgWpgmDAGm: a candidate that keeps
    // failing must stop being redispatched, but a sibling candidate in the same
    // pass — standing in for a healthy repository — must still be attempted.
    let mut streaks = BTreeMap::new();
    let mut spawned = Vec::new();
    let mut logged = Vec::new();
    let tripped = execute_plan(
        decisions(&["broken", "healthy"]),
        1,
        &mut streaks,
        |candidate| {
            spawned.push(candidate.task_id.clone());
            if candidate.task_id == "broken" {
                Err(std::io::Error::other("spawn failed").into())
            } else {
                Ok(SpawnOutcome::Spawned)
            }
        },
        |kind, candidate, reason| {
            logged.push((kind, candidate.task_id.clone(), reason.to_string()));
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(spawned, ["broken", "healthy"]);
    assert_eq!(
        tripped.into_iter().map(|c| c.task_id).collect::<Vec<_>>(),
        ["broken"]
    );
    // "healthy" spawning successfully produces no entry through this hook (its
    // dispatch is logged inside `spawn_candidate` itself); "broken" logs the
    // failed attempt and then the scoped trip, never a global one.
    assert_eq!(
        logged,
        [
            (
                DecisionKind::Hold,
                "broken".to_string(),
                "spawn_failed:io error: spawn failed".to_string()
            ),
            (
                DecisionKind::CircuitOpen,
                "broken".to_string(),
                "candidate_circuit_open".to_string()
            ),
        ]
    );
    // The tripped candidate's streak is consumed on trip, not left to re-trip
    // every pass once it is skip-listed by the caller.
    assert!(!streaks.contains_key("broken"));
}

#[test]
fn a_held_candidate_spends_no_failure_budget() {
    // A duplicate dispatch the pre-spawn re-check catches is not a spawn fault:
    // counting it would let a stalled merge queue trip a candidate's circuit.
    let mut streaks = BTreeMap::new();
    let mut logged = Vec::new();
    let tripped = execute_plan(
        decisions(&["1", "2"]),
        1,
        &mut streaks,
        |_| Ok(SpawnOutcome::Held("active_worker".into())),
        |kind, candidate, reason| {
            logged.push((kind, candidate.task_id.clone(), reason.to_string()));
            Ok(())
        },
    )
    .unwrap();
    assert!(tripped.is_empty());
    assert!(streaks.is_empty());
    assert_eq!(
        logged,
        [
            (DecisionKind::Hold, "1".to_string(), "active_worker".into()),
            (DecisionKind::Hold, "2".to_string(), "active_worker".into()),
        ]
    );
}

#[test]
fn a_branch_conflict_hold_is_reported_the_same_way_on_every_pass() {
    // Reproduces the 60-second loop from dropr:_ord_VtFSIiLgWpgmDAGm at the
    // execute_plan boundary: a candidate the pre-spawn check holds for a
    // leftover branch never reaches `spawn`'s error arm, on this pass or any
    // later one, and its held reason is identical every time rather than
    // drifting toward a failure.
    let mut streaks = BTreeMap::new();
    for _ in 0..3 {
        let mut logged = Vec::new();
        let tripped = execute_plan(
            decisions(&["stuck"]),
            3,
            &mut streaks,
            |_| Ok(SpawnOutcome::Held("branch_exists:dropr/task-stuck".into())),
            |kind, candidate, reason| {
                logged.push((kind, candidate.task_id.clone(), reason.to_string()));
                Ok(())
            },
        )
        .unwrap();
        assert!(tripped.is_empty());
        assert_eq!(
            logged,
            [(
                DecisionKind::Hold,
                "stuck".to_string(),
                "branch_exists:dropr/task-stuck".to_string()
            )]
        );
    }
    assert!(streaks.is_empty());
}

/// dropr:ZJd6VtMdhDsD39-oeoq_L: `SpawnOutcome::Escalated` reaches the operator
/// through `DecisionKind::Escalate`, not `Hold` — logging it the same way a
/// routine hold is logged would leave it buried in the digest with every
/// other steady-state reason.
#[test]
fn an_escalated_outcome_is_logged_as_an_escalation_not_a_hold() {
    let mut streaks = BTreeMap::new();
    let mut logged = Vec::new();
    let tripped = execute_plan(
        decisions(&["stuck"]),
        3,
        &mut streaks,
        |_| {
            Ok(SpawnOutcome::Escalated(
                "branch_exists:dropr/task-stuck".into(),
            ))
        },
        |kind, candidate, reason| {
            logged.push((kind, candidate.task_id.clone(), reason.to_string()));
            Ok(())
        },
    )
    .unwrap();
    assert!(tripped.is_empty());
    // Not a spawn fault: an escalated candidate never spends the failure
    // budget any more than a held one does.
    assert!(streaks.is_empty());
    assert_eq!(
        logged,
        [(
            DecisionKind::Escalate,
            "stuck".to_string(),
            "branch_exists:dropr/task-stuck".to_string()
        )]
    );
}

#[test]
fn the_failure_budget_accumulates_per_candidate_across_an_interleaved_success() {
    // The exact dilution the global counter suffered: `healthy` dispatches
    // successfully on every pass while `broken` keeps failing. With a scoped
    // budget `healthy`'s successes must not reset `broken`'s streak, so
    // `broken` still trips on schedule.
    let mut streaks = BTreeMap::new();
    let threshold = 3;
    for pass in 1..=3 {
        let mut spawned = Vec::new();
        let tripped = execute_plan(
            decisions(&["broken", "healthy"]),
            threshold,
            &mut streaks,
            |candidate| {
                spawned.push(candidate.task_id.clone());
                if candidate.task_id == "broken" {
                    Err(std::io::Error::other("spawn failed").into())
                } else {
                    Ok(SpawnOutcome::Spawned)
                }
            },
            |_, _, _| Ok(()),
        )
        .unwrap();
        assert_eq!(spawned, ["broken", "healthy"], "pass {pass}");
        if pass < 3 {
            assert!(tripped.is_empty(), "pass {pass} tripped early");
        } else {
            assert_eq!(
                tripped.into_iter().map(|c| c.task_id).collect::<Vec<_>>(),
                ["broken"],
                "pass {pass} did not trip on schedule"
            );
        }
    }
    assert!(!streaks.contains_key("broken"));
}
