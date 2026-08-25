use super::*;
use crate::model::{RepoNode, Status};
use chrono::Local;

fn agent(id: &str) -> AgentNode {
    let now = Local::now();
    AgentNode {
        id: id.to_string(),
        parent_agent_id: None,
        title: format!("#{id}"),
        task_number: None,
        worktree_path: format!("/worktrees/{id}").into(),
        branch: format!("feature/{id}"),
        base_commit: String::new(),
        program: "codex".to_string(),
        spawned_by_version: None,
        claude_session_id: None,
        profile: None,
        tmux_session: format!("robco_{id}"),
        created_at: now,
        updated_at: now,
        status: Status::Running,
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
    }
}

fn repo(path: &str, agents: Vec<AgentNode>) -> RepoNode {
    RepoNode {
        path: path.into(),
        name: path.to_string(),
        remote_url: None,
        pinned: false,
        agents,
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
    }
}

#[test]
fn a_target_with_no_entry_and_no_registry_match_stays_unlandable() {
    let mut ledger = Ledger::default();
    let now = Utc::now();
    assert!(ensure_landable(&mut ledger, "ghost", None, now).is_none());
    let registry = Registry {
        version: 1,
        repos: vec![repo("/repo", vec![agent("other")])],
    };
    assert!(ensure_landable(&mut ledger, "ghost", Some(&registry), now).is_none());
    assert!(ledger.entries.is_empty());
}

/// dropr:874's exact shape: the operator pressed `m` on a worker the
/// daemon never adopted at all yet. `ensure_landable` adopts it on the
/// spot instead of leaving `m` with nothing to act on.
#[test]
fn a_target_with_no_entry_but_a_registry_match_is_adopted_on_the_spot() {
    let mut ledger = Ledger::default();
    let registry = Registry {
        version: 1,
        repos: vec![repo("/repo", vec![agent("worker-1")])],
    };
    let now = Utc::now();

    let entry = ensure_landable(&mut ledger, "worker-1", Some(&registry), now)
        .expect("agent exists in the registry");
    assert_eq!(entry.phase, LedgerPhase::Dispatched);
    assert_eq!(entry.dispatched_at, now);
    assert_eq!(entry.repo, "/repo");
    // `agent("worker-1")` carries no `task_number` — a manually-created or
    // truly foreign agent — so the adopted entry must not be handed a dropr
    // task id made up from its own agent id (dropr:531).
    assert_eq!(entry.dropr_task_id, None);
    assert_eq!(ledger.entries.len(), 1);
}

/// An agent Overseer itself dispatched still carries its `task_number` even
/// when the daemon lost track of it and has to adopt it fresh from the
/// registry — so the adopted entry recovers its real dropr task rather than
/// losing it the way `agent.id` alone would (dropr:531).
#[test]
fn an_adopted_agent_with_a_task_number_keeps_its_dropr_task() {
    let mut ledger = Ledger::default();
    let mut numbered = agent("worker-1");
    numbered.task_number = Some("333".into());
    let registry = Registry {
        version: 1,
        repos: vec![repo("/repo", vec![numbered])],
    };
    let now = Utc::now();

    let entry = ensure_landable(&mut ledger, "worker-1", Some(&registry), now)
        .expect("agent exists in the registry");
    assert_eq!(entry.dropr_task_id.as_deref(), Some("#333"));
}

/// dropr:874's other half: the ledger already carries an entry, but it
/// settled `Failed` from a stuck-timeout false positive. `m` must still
/// be able to act on it.
#[test]
fn a_failed_entry_with_a_pull_request_is_revived_to_pr_opened() {
    let mut ledger = Ledger::default();
    ledger.entries.push(LedgerEntry {
        phase: LedgerPhase::Failed,
        pr_url: Some("https://github.test/pull/1".into()),
        worker_escalated: true,
        merge_hold_cap_escalated: true,
        settled_at: Some(Utc::now()),
        ..blank_entry()
    });
    let now = Utc::now();

    let entry = ensure_landable(&mut ledger, "worker-1", None, now).unwrap();
    assert_eq!(entry.phase, LedgerPhase::PrOpened);
    assert_eq!(entry.dispatched_at, now);
    assert_eq!(entry.settled_at, None);
    assert!(!entry.worker_escalated);
    assert!(!entry.merge_hold_cap_escalated);
}

/// An escalated entry with no pull request recorded yet revives to
/// `Dispatched` rather than a `PrOpened` the gate has nothing to read for.
#[test]
fn an_escalated_entry_with_no_pull_request_is_revived_to_dispatched() {
    let mut ledger = Ledger::default();
    ledger.entries.push(LedgerEntry {
        phase: LedgerPhase::Escalated,
        ..blank_entry()
    });
    let now = Utc::now();

    let entry = ensure_landable(&mut ledger, "worker-1", None, now).unwrap();
    assert_eq!(entry.phase, LedgerPhase::Dispatched);
}

/// A genuinely merged entry is never revived — that settlement is real,
/// and flipping it back would replay cleanup on a worktree already gone.
#[test]
fn a_merged_entry_is_left_untouched() {
    let mut ledger = Ledger::default();
    ledger.entries.push(LedgerEntry {
        phase: LedgerPhase::Merged,
        ..blank_entry()
    });
    let now = Utc::now();

    let entry = ensure_landable(&mut ledger, "worker-1", None, now).unwrap();
    assert_eq!(entry.phase, LedgerPhase::Merged);
}

fn blank_entry() -> LedgerEntry {
    LedgerEntry {
        task_id: "worker-1".into(),
        dropr_task_id: None,
        display_id: "#1".into(),
        repo: "/repo".into(),
        agent_id: "worker-1".into(),
        branch: "feature/worker-1".into(),
        phase: LedgerPhase::Dispatched,
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
        worker_finished_at: None,
        approval_dropped: None,
    }
}
