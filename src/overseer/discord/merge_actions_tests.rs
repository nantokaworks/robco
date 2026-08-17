use chrono::Utc;

use super::*;

fn entry(task_id: &str, display_id: &str, phase: LedgerPhase) -> LedgerEntry {
    LedgerEntry {
        task_id: task_id.into(),
        display_id: display_id.into(),
        repo: "/repo".into(),
        agent_id: "agent-1".into(),
        branch: "branch".into(),
        phase,
        dispatched_at: Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: Some("https://github.com/acme/widgets/pull/1".into()),
        branch_updates: 0,
        merge_judge_primes: 0,
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
        escalation_notified_reason: None,
        escalation_notified_head: None,
        worker_escalated: false,
        operator_override: None,
    }
}

#[test]
fn finds_by_task_id_or_display_id() {
    let ledger = Ledger {
        entries: vec![entry("task-1", "#1", LedgerPhase::Escalated)],
        ..Default::default()
    };
    assert_eq!(
        find_ledger_entry(&ledger, "task-1").unwrap().task_id,
        "task-1"
    );
    assert_eq!(find_ledger_entry(&ledger, "#1").unwrap().task_id, "task-1");
    assert!(find_ledger_entry(&ledger, "missing").is_err());
}

#[test]
fn find_agent_looks_up_by_id_across_repos() {
    use chrono::Local;

    use crate::model::{AgentNode, ManagementMode, RepoNode, Status};

    let registry = Registry {
        version: 1,
        repos: vec![RepoNode {
            path: "/repo".into(),
            name: "widgets".into(),
            remote_url: None,
            pinned: false,
            management: ManagementMode::Auto,
            agents: vec![AgentNode {
                management: ManagementMode::Auto,
                id: "agent-1".into(),
                parent_agent_id: None,
                title: "task".into(),
                task_number: None,
                worktree_path: "/repo-wt".into(),
                branch: "task".into(),
                base_commit: "abc".into(),
                program: "codex".into(),
                claude_session_id: None,
                profile: None,
                tmux_session: "robco-task".into(),
                created_at: Local::now(),
                updated_at: Local::now(),
                status: Status::Idle,
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
            }],
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
    assert!(find_agent(&registry, "agent-1").is_ok());
    assert!(find_agent(&registry, "missing").is_err());
}
