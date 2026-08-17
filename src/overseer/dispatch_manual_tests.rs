//! How `ManagementMode::Manual` meets the dispatch gate: the task a Manual
//! worker owns is never re-dispatched, and the worker still occupies capacity.

use super::*;

#[test]
fn manual_worker_is_excluded_and_auto_worker_is_included() {
    let mut ledger = Ledger::default();
    let mut manual = entry(LedgerPhase::Failed);
    manual.task_id = "manual-task".into();
    manual.display_id = "#1".into();
    manual.agent_id = "manual-agent".into();
    let mut auto = entry(LedgerPhase::Failed);
    auto.task_id = "auto-task".into();
    auto.display_id = "#2".into();
    auto.agent_id = "auto-agent".into();
    ledger.entries.extend([manual, auto]);
    let modes = HashMap::from([
        ("manual-agent".into(), ManagementMode::Manual),
        ("auto-agent".into(), ManagementMode::Auto),
    ]);
    let candidates = [
        Candidate {
            task_id: "manual-task".into(),
            display_id: "#1".into(),
            title: "manual".into(),
            repo: "/manual".into(),
            author: "allowed".into(),
            priority: "medium".into(),
            workspace: "workspace-1".into(),
            priority_score: None,
            status: "open".into(),
            parent_task_id: None,
        },
        Candidate {
            task_id: "auto-task".into(),
            display_id: "#2".into(),
            title: "auto".into(),
            repo: "/auto".into(),
            author: "allowed".into(),
            priority: "medium".into(),
            workspace: "workspace-1".into(),
            priority_score: None,
            status: "open".into(),
            parent_task_id: None,
        },
    ];

    let plan = plan_dispatch(
        &OverseerConfig::default(),
        &ledger,
        &candidates,
        now(),
        &modes,
    );
    assert_eq!(plan.decisions[0].reason, "manual");
    assert!(!plan.decisions[0].dispatch);
    assert_eq!(plan.decisions[1].reason, "ready");
    assert!(plan.decisions[1].dispatch);
}

#[test]
fn a_live_manual_worker_still_consumes_capacity() {
    // Manual suppresses Overseer *intervention*, not occupancy: the worker still
    // holds a worktree, a branch, and a tmux session in its repository. Exempting
    // it from the primary slot would let one `g` toggle free a slot the
    // resources never released.
    let modes = HashMap::from([("manual-agent".to_string(), ManagementMode::Manual)]);
    let mut ledger = Ledger::default();
    let mut manual = entry(LedgerPhase::Working);
    manual.task_id = "manual-task".into();
    manual.display_id = "#9".into();
    manual.agent_id = "manual-agent".into();
    manual.repo = "/repo".into();
    ledger.entries.push(manual);

    let plan = plan_dispatch(
        &OverseerConfig::default(),
        &ledger,
        &[candidate("/repo")],
        now(),
        &modes,
    );
    // The gate counted the very worker `robco overseer status` reports.
    assert_eq!(ledger.active_workers().count, 1);
    assert_eq!(ledger.active_workers().repos.get("/repo").copied(), Some(1));
    assert_eq!(plan.decisions[0].reason, "primary_slot_taken");
    assert!(!plan.decisions[0].dispatch);
}

#[test]
fn a_manual_workers_own_task_is_never_redispatched() {
    // Counting Manual workers toward the caps must not weaken the protection the
    // mode exists for: the task a Manual worker owns stays off the dispatch list
    // even when a secondary slot is open.
    let mut ledger = Ledger::default();
    let mut manual = entry(LedgerPhase::Working);
    manual.task_id = "task-1".into();
    manual.display_id = "#1".into();
    manual.agent_id = "manual-agent".into();
    ledger.entries.push(manual);
    let modes = HashMap::from([("manual-agent".to_string(), ManagementMode::Manual)]);

    let plan = plan_dispatch(
        &OverseerConfig {
            parallel_limit: 5,
            ..OverseerConfig::default()
        },
        &ledger,
        &[candidate("/repo")],
        now(),
        &modes,
    );
    assert_eq!(plan.decisions[0].reason, "manual");
    assert!(!plan.decisions[0].dispatch);
}
