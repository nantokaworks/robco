use super::*;
use crate::overseer::ledger::MergeApproval;

fn unrequested_entry(pr_url: Option<&str>) -> LedgerEntry {
    LedgerEntry {
        task_id: "task".into(),
        display_id: "#1".into(),
        repo: "/repo".into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
        phase: LedgerPhase::PrOpened,
        dispatched_at: chrono::Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: pr_url.map(str::to_owned),
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        manual_merge_skip: None,
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
    }
}

fn empty_registry() -> Registry {
    Registry {
        version: 1,
        repos: Vec::new(),
    }
}

// dropr:500 — a pull request nobody asked about is never gated.
#[test]
fn an_unrequested_pull_request_is_left_untouched_by_the_merge_pass() {
    let mut entry = unrequested_entry(Some("https://pr/1"));
    let before = entry.clone();
    let config = Config::default();
    let cache = ProtectionCache::default();
    let registry = empty_registry();
    let work = RepoWork {
        repo: "/repo".into(),
        entries: vec![&mut entry],
        settling: None,
    };

    let outcome = run(work, &config, &cache, &registry, 10, 5).unwrap();

    assert!(!outcome.merged);
    // No gate ran, no `gh` call happened, nothing about the entry changed —
    // in particular no decision was charged against its hold budget.
    assert_eq!(entry, before);
}

// dropr:500 — a merge the operator asked for that cannot proceed still
// escalates to the operator once its hold budget is spent. `pr_url: None`
// drives the gate's own `missing_pr_url` hold without shelling out to `gh`.
#[test]
fn a_requested_pull_request_that_cannot_proceed_escalates_once_its_hold_budget_is_spent() {
    let mut entry = unrequested_entry(None);
    entry.merge_approval = Some(MergeApproval {
        head: "abc123".into(),
        granted_at: chrono::Utc::now(),
    });
    let mut config = Config::default();
    config.overseer.max_merge_holds = 0;
    let cache = ProtectionCache::default();
    let registry = empty_registry();
    let work = RepoWork {
        repo: "/repo".into(),
        entries: vec![&mut entry],
        settling: None,
    };

    run(work, &config, &cache, &registry, 10, 5).unwrap();

    assert_eq!(entry.phase, LedgerPhase::Escalated);
}

#[test]
fn a_manual_worker_never_reaches_the_gate_or_its_recovery() {
    // Manual suppresses Overseer intervention entirely: the worker belongs to a
    // human, so neither the merge nor a merge-failure handback is Overseer's to
    // initiate. This gate is what merge recovery relies on to stay out.
    let mut entry = LedgerEntry {
        task_id: "task".into(),
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
        manual_merge_skip: None,
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
    };
    let now = chrono::Local::now();
    let agent = |management| crate::model::AgentNode {
        id: "agent".into(),
        parent_agent_id: Some(crate::overseer::OVERSEER_AGENT_ID.into()),
        management,
        title: "worker".into(),
        task_number: None,
        worktree_path: "/tmp/agent".into(),
        branch: "branch".into(),
        base_commit: String::new(),
        program: "claude".into(),
        claude_session_id: None,
        profile: None,
        tmux_session: "agent".into(),
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
    let registry = |management| Registry {
        version: 1,
        repos: vec![crate::model::RepoNode {
            path: "/repo".into(),
            name: "repo".into(),
            remote_url: None,
            pinned: false,
            management: crate::model::ManagementMode::Auto,
            agents: vec![agent(management)],
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
    };

    assert!(worker_is_auto(
        &entry,
        &registry(crate::model::ManagementMode::Auto)
    ));
    assert!(!worker_is_auto(
        &entry,
        &registry(crate::model::ManagementMode::Manual)
    ));
    // A worker the registry no longer lists is not evidence of a human owner.
    entry.agent_id = "gone".into();
    assert!(worker_is_auto(
        &entry,
        &registry(crate::model::ManagementMode::Manual)
    ));
}

#[test]
fn a_repo_opted_out_of_the_overseer_blocks_auto_merge_for_every_worker_in_it() {
    // #306: a repo the operator switched out of Overseer management should not
    // have its pull requests merged automatically either, even for a worker
    // still individually marked Auto — the repo-level toggle is the coarser
    // gate and wins.
    let entry = LedgerEntry {
        task_id: "task".into(),
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
        manual_merge_skip: None,
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
    };
    let now = chrono::Local::now();
    let agent = crate::model::AgentNode {
        id: "agent".into(),
        parent_agent_id: Some(crate::overseer::OVERSEER_AGENT_ID.into()),
        management: crate::model::ManagementMode::Auto,
        title: "worker".into(),
        task_number: None,
        worktree_path: "/tmp/agent".into(),
        branch: "branch".into(),
        base_commit: String::new(),
        program: "claude".into(),
        claude_session_id: None,
        profile: None,
        tmux_session: "agent".into(),
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
    let registry = |management| Registry {
        version: 1,
        repos: vec![crate::model::RepoNode {
            path: "/repo".into(),
            name: "repo".into(),
            remote_url: None,
            pinned: false,
            management,
            agents: vec![agent.clone()],
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
    };

    assert!(worker_is_auto(
        &entry,
        &registry(crate::model::ManagementMode::Auto)
    ));
    assert!(!worker_is_auto(
        &entry,
        &registry(crate::model::ManagementMode::Manual)
    ));
}
