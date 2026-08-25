//! `dispatch`'s own behaviour against a busy worker session — split out of
//! `merge_recovery_tests.rs` to keep that file under this project's source
//! file size limit, since these need a real tmux session the pure-logic
//! tests do not.

use super::*;
use crate::overseer::ledger::LedgerPhase;

fn entry() -> LedgerEntry {
    LedgerEntry {
        task_id: "task".into(),
        dropr_task_id: None,
        display_id: "#1".into(),
        repo: "/repo".into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
        phase: LedgerPhase::PrOpened,
        dispatched_at: chrono::Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: Some("https://pr/1".into()),
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
        worker_finished_at: None,
        approval_dropped: None,
    }
}

/// The exact bug dropr:530 exists to stop: refusing to type over a busy
/// session is correct (it would corrupt whatever the worker is composing),
/// but the notice must not then be dropped. `dispatch` must hold it instead
/// of sending, on the very first attempt — not only on a retry of an
/// earlier unconfirmed send.
#[test]
fn a_busy_session_withholds_the_handback_instead_of_sending_into_it() {
    let session = format!("robco-test-dispatch-busy-{}", std::process::id());
    if crate::tmux::new_session(&session, &std::env::temp_dir(), "sh", &[]).is_err() {
        // No usable tmux in this environment — the pure-logic tests in
        // `merge_recovery_pending_tests.rs` already cover the branches this
        // test would exercise.
        return;
    }
    let _ = crate::tmux::send_literal_text(&session, "esc to interrupt");

    let mut entry = entry();
    // The state right after `plan` charged this poll's attempt. Nothing has
    // ever failed to confirm for this head — this is a first attempt.
    entry.merge_recovery.charged = 1;
    entry.merge_recovery.head = Some("sha-1".into());
    entry.merge_recovery.base = Some("base-1".into());

    let registry = registry_with_session(&session);
    dispatch(&mut entry, "merge_state:dirty", &registry, None, 2).unwrap();

    // Un-charged, same as any other attempt that could not send anything.
    assert_eq!(entry.merge_recovery.charged, 0);
    assert_eq!(entry.merge_recovery.head, None);
    assert_eq!(entry.merge_recovery.base, None);
    // Remembered so a later pass retries it instead of the notice quietly
    // never arriving.
    let pending = entry.merge_recovery.pending.as_ref().unwrap();
    assert_eq!(pending.reason, "merge_state:dirty");
    assert_eq!(pending.head, "sha-1");
    assert_eq!(pending.attempts, 1);
    // A hold, not an escalation or a dispatched handback: the entry stays
    // exactly where it started.
    assert_eq!(entry.phase, LedgerPhase::PrOpened);

    let _ = crate::tmux::kill_session(&session);
}

/// A worker that is never idle must not be retried forever: once the busy
/// bound is spent, `dispatch` escalates and names the entry and reason
/// instead of holding a fourth time.
#[test]
fn a_session_that_never_frees_up_escalates_once_the_bound_is_spent() {
    let session = format!("robco-test-dispatch-busy-cap-{}", std::process::id());
    if crate::tmux::new_session(&session, &std::env::temp_dir(), "sh", &[]).is_err() {
        return;
    }
    let _ = crate::tmux::send_literal_text(&session, "esc to interrupt");

    let mut entry = entry();
    let registry = registry_with_session(&session);
    let max_recoveries = 3;

    // `plan` recharges the same head/base every pass because `dispatch`
    // refunds it on the way out — mirrored here without going through
    // `plan` itself, since the session's busy state is what this test
    // exercises.
    for _ in 0..max_recoveries - 1 {
        entry.merge_recovery.head = Some("sha-1".into());
        entry.merge_recovery.base = Some("base-1".into());
        dispatch(
            &mut entry,
            "merge_state:dirty",
            &registry,
            None,
            max_recoveries,
        )
        .unwrap();
        assert_eq!(entry.phase, LedgerPhase::PrOpened);
    }

    entry.merge_recovery.head = Some("sha-1".into());
    entry.merge_recovery.base = Some("base-1".into());
    dispatch(
        &mut entry,
        "merge_state:dirty",
        &registry,
        None,
        max_recoveries,
    )
    .unwrap();

    assert_eq!(entry.phase, LedgerPhase::Escalated);
    // Abandoned, not left pending — silence at the end of the retries is the
    // same bug in a slower form.
    assert!(entry.merge_recovery.pending.is_none());

    let _ = crate::tmux::kill_session(&session);
}

fn registry_with_session(session: &str) -> Registry {
    let now = chrono::Local::now();
    let agent = crate::model::AgentNode {
        id: "agent".into(),
        parent_agent_id: Some(crate::overseer::OVERSEER_AGENT_ID.into()),
        title: "worker".into(),
        task_number: None,
        worktree_path: "/tmp/agent".into(),
        branch: "branch".into(),
        base_commit: String::new(),
        program: "claude".into(),
        spawned_by_version: None,
        claude_session_id: None,
        profile: None,
        tmux_session: session.into(),
        created_at: now,
        updated_at: now,
        status: crate::model::Status::Running,
        worktree_missing: false,
        merge_error: None,
        last_capture: None,
        last_spinner: None,
        last_change_at: None,
        last_auto_accept_at: None,
        shell_working: false,
        mcp_active: false,
        pane_pid: None,
        tracked_command: None,
        subagents: Vec::new(),
        children: Vec::new(),
    };
    Registry {
        version: 1,
        repos: vec![crate::model::RepoNode {
            path: "/repo".into(),
            name: "repo".into(),
            remote_url: None,
            pinned: false,
            agents: vec![agent],
            dropr: None,
            dropr_tasks: crate::dropr::DroprTaskFetch::default(),
            main_status: None,
            main_last_capture: None,
            main_last_spinner: None,
            main_last_change_at: None,
            main_shell_working: false,
            main_mcp_active: false,
            main_pane_pid: None,
            main_tracked_command: None,
            main_subagents_active: 0,
            main_behind_origin: None,
            checkout_state: None,
        }],
    }
}
