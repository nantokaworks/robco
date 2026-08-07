use super::*;
use crate::overseer::ledger::LedgerEntry;

fn registry_with(parent: Option<&str>, management: &str) -> Registry {
    serde_json::from_value(serde_json::json!({
        "version": 1,
        "repos": [{
            "path": "/repo",
            "name": "repo",
            "remote_url": null,
            "agents": [{
                "id": "manual-worker",
                "parent_agent_id": parent,
                "management": management,
                "title": "#154",
                "worktree_path": "/repo/worker",
                "branch": "task-154",
                "base_commit": "",
                "program": "codex",
                "tmux_session": "robco_repo_task-154",
                "created_at": "2026-07-18T00:00:00+09:00",
                "updated_at": "2026-07-18T00:00:00+09:00"
            }]
        }]
    }))
    .unwrap()
}

fn ledger_for(agent_id: &str) -> Ledger {
    Ledger {
        entries: vec![LedgerEntry {
            task_id: "task-154".into(),
            display_id: "#154".into(),
            repo: "/repo".into(),
            agent_id: agent_id.into(),
            branch: "task-154".into(),
            phase: LedgerPhase::Working,
            dispatched_at: Utc::now(),
            settled_at: None,
            retries: 0,
            pr_url: None,
            branch_updates: 0,
            merge_recovery: Default::default(),
            merge_hold: Default::default(),
            manual_merge_skip: None,
            merge_judge_fail_safes: 0,
            merge_hold_cap_escalated: false,
            merge_hold_rechecks: 0,
            merge_hold_recheck_reason: None,
            merge_hold_recheck_head: None,
            prerequisite_wait: None,
            merge_hold_stuck_notified: false,
            worker_escalated: false,
            operator_override: None,
        }],
        ..Ledger::default()
    }
}

/// The state `g` leaves behind on the `Manual -> Unmanaged` step: ownership is
/// cleared while the registry row survives carrying `Manual`.
#[test]
fn detached_worker_is_reported_detached() {
    let registry = registry_with(None, "manual");

    assert_eq!(
        detached_agents(&ledger_for("manual-worker"), &registry),
        vec!["manual-worker".to_string()]
    );
}

#[test]
fn overseer_children_and_unregistered_agents_are_not_detached() {
    let registry = registry_with(Some(crate::overseer::OVERSEER_AGENT_ID), "manual");

    assert!(detached_agents(&ledger_for("manual-worker"), &registry).is_empty());
    // An agent that left the registry entirely is dead, not detached: dropping
    // its entry here would hide the session-death path that fails it.
    assert!(detached_agents(&ledger_for("gone-worker"), &registry).is_empty());
}

#[test]
fn manual_overseer_children_are_not_adopted() {
    let registry = registry_with(Some(crate::overseer::OVERSEER_AGENT_ID), "manual");
    let mut ledger = Ledger::default();

    adopt_registry_children_from(&mut ledger, &registry);

    assert!(ledger.entries.is_empty());
}
