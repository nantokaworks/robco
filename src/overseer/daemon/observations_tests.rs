use super::*;
use crate::overseer::ledger::LedgerEntry;

fn registry_with(parent: Option<&str>) -> Registry {
    serde_json::from_value(serde_json::json!({
        "version": 1,
        "repos": [{
            "path": "/repo",
            "name": "repo",
            "remote_url": null,
            "agents": [{
                "id": "manual-worker",
                "parent_agent_id": parent,
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
            merge_hold_cap_escalated: false,
            merge_hold_rechecks: 0,
            merge_hold_recheck_reason: None,
            merge_hold_recheck_head: None,
            prerequisite_wait: None,
            merge_hold_stuck_notified: false,
            escalation_notified_reason: None,
            escalation_notified_head: None,
            worker_escalated: false,
            operator_override: None,
            merge_approval: None,
            pr_facts: None,
        }],
        ..Ledger::default()
    }
}

/// A worker whose registry row survives with a parent that is not the
/// Overseer's own id is reported detached — including a row with no parent
/// at all.
#[test]
fn detached_worker_is_reported_detached() {
    let registry = registry_with(None);

    assert_eq!(
        detached_agents(&ledger_for("manual-worker"), &registry),
        vec!["manual-worker".to_string()]
    );
}

#[test]
fn overseer_children_and_unregistered_agents_are_not_detached() {
    let registry = registry_with(Some(crate::overseer::OVERSEER_AGENT_ID));

    assert!(detached_agents(&ledger_for("manual-worker"), &registry).is_empty());
    // An agent that left the registry entirely is dead, not detached: dropping
    // its entry here would hide the session-death path that fails it.
    assert!(detached_agents(&ledger_for("gone-worker"), &registry).is_empty());
}

/// A worker with no parent at all is not the Overseer's to adopt. Every
/// creation path (`agent::creation::enroll_with_overseer`, `adopt_worktree`)
/// sets `parent_agent_id` to the Overseer's own id up front, so a `None`
/// parent here means the worker genuinely belongs to nobody.
#[test]
fn agent_with_no_parent_is_not_adopted() {
    let registry = registry_with(None);
    let mut ledger = Ledger::default();

    adopt_registry_children_from(&mut ledger, &registry);

    assert!(ledger.entries.is_empty());
}

/// Pins dropr:489: adoption used to run once at daemon startup, so a worker
/// started while the daemon was already running was never picked up until a
/// restart. It must run again on a later pass too, once the registry gains
/// the worker.
#[test]
fn worker_started_after_the_daemon_booted_is_adopted_on_a_later_pass() {
    let empty_registry = Registry {
        version: 1,
        repos: Vec::new(),
    };
    let mut ledger = Ledger::default();
    // First pass: the worker has not started yet, so the registry has no repos.
    adopt_registry_children_from(&mut ledger, &empty_registry);
    assert!(ledger.entries.is_empty());

    // The worker starts while the daemon keeps running. A later pass reads
    // the registry again and must still find it, with no restart in between.
    let registry_with_worker = registry_with(Some(crate::overseer::OVERSEER_AGENT_ID));
    adopt_registry_children_from(&mut ledger, &registry_with_worker);

    assert_eq!(ledger.entries.len(), 1);
    assert_eq!(ledger.entries[0].agent_id, "manual-worker");
}

#[test]
fn an_agent_already_in_the_ledger_is_not_adopted_twice() {
    let registry = registry_with(Some(crate::overseer::OVERSEER_AGENT_ID));
    let mut ledger = Ledger::default();

    adopt_registry_children_from(&mut ledger, &registry);
    // A repeat call, as every later pass makes, must not duplicate the entry.
    adopt_registry_children_from(&mut ledger, &registry);

    assert_eq!(ledger.entries.len(), 1);
}

/// Pins dropr:489's third acceptance point: retention drops a settled entry
/// only once its worker has left the registry, and adoption only re-adds an
/// entry for an agent the registry still lists. So once a settled entry is
/// gone, calling adoption again on the same (now agent-less) registry must
/// not bring it back.
#[test]
fn a_settled_entry_retention_pruned_does_not_come_back_next_pass() {
    use crate::overseer::ledger::LedgerPhase;
    use crate::overseer::monitor::{Observations, TASK_ABSENT, TaskObservation};

    let empty_registry = registry_with(Some(crate::overseer::OVERSEER_AGENT_ID));
    let mut ledger = Ledger::default();
    adopt_registry_children_from(&mut ledger, &empty_registry);
    ledger.entries[0].phase = LedgerPhase::Escalated;
    ledger.entries[0].task_id = "task-154".into();

    // The worker's registry row is gone by the time this pass runs: nothing
    // lists it as registered, and its dropr task is confirmed absent too.
    let registry_without_worker = Registry {
        version: 1,
        repos: Vec::new(),
    };
    let observations = Observations {
        tasks: vec![TaskObservation {
            task_id: "task-154".into(),
            state: TASK_ABSENT.into(),
            updated_at: None,
        }],
        ..Observations::default()
    };
    super::super::retention::prune_pass(&mut ledger, &[], &observations, Utc::now(), 100).unwrap();
    assert!(ledger.entries.is_empty());

    // Next pass: adoption reads the same, now-agent-less registry. There is
    // nothing left to resurrect.
    adopt_registry_children_from(&mut ledger, &registry_without_worker);
    assert!(ledger.entries.is_empty());
}

/// Regression for the bare `={session}` target: on tmux 3.7, `display-message`
/// against it exits 0 and prints an empty string for `#{session_activity}`,
/// so `tmux_activity` returned `None` for every live session that was ever
/// probed. The anchored `={session}:` target must resolve a real timestamp.
#[test]
fn tmux_activity_reads_a_real_sessions_activity_time() {
    let session = format!("robco-test-tmux-activity-{}", std::process::id());
    if crate::tmux::new_session(&session, &std::env::temp_dir(), "sh", &[]).is_err() {
        // No usable tmux in this environment — nothing more to assert here.
        return;
    }
    let result = tmux_activity(&session);
    let _ = crate::tmux::kill_session(&session);
    assert!(
        result.is_ok(),
        "expected a real session_activity reading, got {result:?}"
    );
}

#[test]
fn tmux_activity_reports_a_fault_for_a_missing_session() {
    let result = tmux_activity("robco-test-tmux-activity-missing-session");

    assert!(result.is_err());
}
