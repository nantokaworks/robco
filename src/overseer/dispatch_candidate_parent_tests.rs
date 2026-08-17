use super::*;

/// dropr:yD5Gf6TX23VMvuSLFsmvO defect 2: a parent task dispatched under RUN
/// covers its whole subtree, but each subtask stays `open` and unclaimed in
/// dropr while that run is live — without this check, a free worker slot
/// dispatches the subtask separately while the parent's worker is still
/// building the exact same change.
#[test]
fn a_live_parent_worker_holds_its_subtask() {
    let mut ledger = Ledger::default();
    let mut parent_run = entry(LedgerPhase::Working);
    parent_run.task_id = "parent-task".into();
    parent_run.display_id = "#431".into();
    parent_run.repo = "/elsewhere".into();
    ledger.entries.push(parent_run);

    let subtask = Candidate {
        parent_task_id: Some("parent-task".into()),
        ..candidate("/repo")
    };
    let plan = plan_dispatch(
        &OverseerConfig::default(),
        &ledger,
        &[subtask],
        now(),
        &HashMap::new(),
    );
    assert_eq!(plan.decisions[0].reason, "parent_worker_active");
    assert!(!plan.decisions[0].dispatch);
}

/// Once the parent's own entry settles into a terminal phase, a subtask the
/// parent's run did not cover must still be dispatchable — the hold names the
/// parent's ledger entry, not the dropr task hierarchy itself, so it must
/// release the moment that entry does.
#[test]
fn a_settled_parent_worker_releases_its_subtask() {
    for phase in [
        LedgerPhase::Merged,
        LedgerPhase::Failed,
        LedgerPhase::Escalated,
    ] {
        let mut ledger = Ledger::default();
        let mut parent_run = entry(phase);
        parent_run.task_id = "parent-task".into();
        parent_run.display_id = "#431".into();
        parent_run.repo = "/elsewhere".into();
        ledger.entries.push(parent_run);

        let subtask = Candidate {
            parent_task_id: Some("parent-task".into()),
            ..candidate("/repo")
        };
        let plan = plan_dispatch(
            &OverseerConfig::default(),
            &ledger,
            &[subtask],
            now(),
            &HashMap::new(),
        );
        assert_eq!(plan.decisions[0].reason, "ready");
        assert!(plan.decisions[0].dispatch);
    }
}

/// A root task (no parent) must never consult this check at all — sanity
/// pin so the ancestor gate cannot start firing for every candidate if
/// `parent_task_id` handling ever regresses to a non-`Option` default.
#[test]
fn a_root_candidate_is_never_held_on_a_parent_worker() {
    let mut ledger = Ledger::default();
    let mut unrelated = entry(LedgerPhase::Working);
    unrelated.task_id = "some-other-task".into();
    unrelated.repo = "/elsewhere".into();
    ledger.entries.push(unrelated);

    let plan = plan_dispatch(
        &OverseerConfig::default(),
        &ledger,
        &[candidate("/repo")],
        now(),
        &HashMap::new(),
    );
    assert_eq!(plan.decisions[0].reason, "ready");
    assert!(plan.decisions[0].dispatch);
}

/// dropr:452 — a subtask whose priority outranks its own parent's must not
/// take the slot while the parent is also a ready candidate this pass, or a
/// RUN dispatch against the parent ends up building the same change twice.
/// `order_candidates` sorts on priority alone, so without this gate the
/// higher-priority subtask would sort ahead of its parent and dispatch
/// first.
#[test]
fn a_higher_priority_subtask_does_not_dispatch_ahead_of_its_ready_parent() {
    let parent = Candidate {
        task_id: "parent-task".into(),
        priority: "low".into(),
        ..candidate("/repo")
    };
    let subtask = Candidate {
        task_id: "subtask".into(),
        priority: "high".into(),
        parent_task_id: Some("parent-task".into()),
        ..candidate("/repo")
    };

    let plan = plan_dispatch(
        &OverseerConfig::default(),
        &Ledger::default(),
        &[subtask, parent],
        now(),
        &HashMap::new(),
    );

    let subtask_decision = plan
        .decisions
        .iter()
        .find(|decision| {
            decision
                .candidate
                .as_ref()
                .is_some_and(|c| c.task_id == "subtask")
        })
        .expect("subtask decision recorded");
    assert_eq!(subtask_decision.reason, "ancestor_candidate");
    assert!(!subtask_decision.dispatch);

    let parent_decision = plan
        .decisions
        .iter()
        .find(|decision| {
            decision
                .candidate
                .as_ref()
                .is_some_and(|c| c.task_id == "parent-task")
        })
        .expect("parent decision recorded");
    assert_eq!(parent_decision.reason, "ready");
    assert!(parent_decision.dispatch);
}

/// Once the parent is no longer among this pass's candidates — dispatched,
/// closed, or held for an unrelated reason — the subtask must be free to
/// dispatch on its own again.
#[test]
fn a_subtask_dispatches_once_its_parent_is_not_a_candidate() {
    let subtask = Candidate {
        task_id: "subtask".into(),
        parent_task_id: Some("parent-task".into()),
        ..candidate("/repo")
    };

    let plan = plan_dispatch(
        &OverseerConfig::default(),
        &Ledger::default(),
        &[subtask],
        now(),
        &HashMap::new(),
    );
    assert_eq!(plan.decisions[0].reason, "ready");
    assert!(plan.decisions[0].dispatch);
}
