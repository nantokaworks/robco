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
