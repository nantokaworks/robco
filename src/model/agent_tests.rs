use super::*;

#[test]
fn merge_error_is_not_persisted() {
    let now = Local::now();
    let agent = AgentNode {
        id: "agent".into(),
        parent_agent_id: None,
        title: "task".into(),
        task_number: None,
        worktree_path: "/tmp/task".into(),
        branch: "task".into(),
        base_commit: String::new(),
        program: "claude".into(),
        spawned_by_version: None,
        claude_session_id: None,
        profile: None,
        tmux_session: "robco_task".into(),
        created_at: now,
        updated_at: now,
        status: Status::Idle,
        worktree_missing: false,
        merge_error: Some("merge failed".into()),
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

    let json = serde_json::to_string(&agent).unwrap();
    assert!(!json.contains("merge_error"));
    let restored: AgentNode = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.merge_error, None);
}
