use super::*;
use crate::overseer::autonomy::ChangeFacts;
use serde_json::json;

#[test]
fn failing_rust_gate_never_invokes_merge_judge() {
    let mut config = Config::default();
    config.overseer.autonomy_level = crate::overseer::autonomy::AutonomyLevel::FullAuto;
    let facts = ChangeFacts {
        facts_known: true,
        ..ChangeFacts::default()
    };
    let mut calls = 0;
    for (protection, checks, candidate_facts) in [
        (false, true, facts),
        (true, false, facts),
        (true, true, ChangeFacts::default()),
    ] {
        let result = judgment_after_gate(protection, checks, &candidate_facts, &config, || {
            calls += 1;
            "called"
        });
        assert_eq!(result, None);
    }
    assert_eq!(calls, 0);
}

#[test]
fn known_low_risk_metadata_reaches_judgment_in_conservative_mode() {
    let config = Config::default();
    let facts = change_facts(
        &json!({
            "additions": 3, "deletions": 1, "changedFiles": 1,
            "files": [{"path": "docs/guide.md"}]
        }),
        0,
        0,
    );
    assert!(facts.facts_known);
    assert_eq!(
        judgment_after_gate(true, true, &facts, &config, || 7),
        Some(7)
    );
}

#[test]
fn incomplete_file_metadata_is_unknown() {
    for value in [
        json!({"additions":1,"deletions":0,"changedFiles":2,"files":[{"path":"a.rs"}]}),
        json!({"additions":1,"deletions":0,"changedFiles":1,"files":[{}]}),
    ] {
        assert!(!change_facts(&value, 0, 0).facts_known);
    }
}

#[test]
fn merge_case_saturates_additions_independently() {
    let entry = crate::overseer::ledger::LedgerEntry {
        task_id: "task".into(),
        display_id: "#1".into(),
        repo: "/repo".into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
        phase: LedgerPhase::PrOpened,
        dispatched_at: chrono::Utc::now(),
        retries: 0,
        pr_url: Some("https://pr/1".into()),
        branch_updates: 0,
        merge_recovery: Default::default(),
        manual_merge_skip: None,
    };
    let value = json!({
        "headRefOid":"new-sha", "additions":u64::MAX, "deletions":u32::MAX,
        "changedFiles":1, "files":[{"path":"src/lib.rs"}]
    });
    let facts = change_facts(&value, 0, 7);
    let case = merge_case(&entry, "https://pr/1", &value);
    assert_eq!(case.additions, u32::MAX);
    assert_eq!(case.deletions, u32::MAX);
    assert_eq!(case.head_sha, "new-sha");
    assert_eq!(facts.llm_calls_today, 7);
}

#[test]
fn veto_escalates_and_cannot_be_selected_again_at_same_revision() {
    let mut entry = crate::overseer::ledger::LedgerEntry {
        task_id: "task".into(),
        display_id: "#1".into(),
        repo: "/repo".into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
        phase: LedgerPhase::PrOpened,
        dispatched_at: chrono::Utc::now(),
        retries: 0,
        pr_url: Some("https://pr/1".into()),
        branch_updates: 0,
        merge_recovery: Default::default(),
        manual_merge_skip: None,
    };
    assert!(!judgment_allows_merge(&mut entry, MergeJudgment::Veto));
    assert_eq!(entry.phase, LedgerPhase::Escalated);
    assert_ne!(entry.phase, LedgerPhase::PrOpened);
}

#[test]
fn a_manual_worker_never_reaches_the_gate_or_its_recovery() {
    // Manual suppresses Overseer intervention entirely: the worker belongs to a
    // human, so neither the merge nor a merge-failure handback is Overseer's to
    // initiate. This gate is what merge recovery relies on to stay out.
    let mut entry = crate::overseer::ledger::LedgerEntry {
        task_id: "task".into(),
        display_id: "#1".into(),
        repo: "/repo".into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
        phase: LedgerPhase::PrOpened,
        dispatched_at: chrono::Utc::now(),
        retries: 0,
        pr_url: Some("https://pr/1".into()),
        branch_updates: 0,
        merge_recovery: Default::default(),
        manual_merge_skip: None,
    };
    let now = chrono::Local::now();
    let agent = |management| crate::model::AgentNode {
        id: "agent".into(),
        parent_agent_id: Some(crate::overseer::OVERSEER_AGENT_ID.into()),
        management,
        title: "worker".into(),
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
            agents: vec![agent(management)],
            dropr: None,
            dropr_tasks: Vec::new(),
            main_status: None,
            main_last_capture: None,
            main_last_spinner: None,
            main_last_change_at: None,
            main_shell_working: false,
            main_mcp_active: false,
            main_pane_pid: None,
            main_tracked_command: None,
            main_subagents_active: 0,
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
