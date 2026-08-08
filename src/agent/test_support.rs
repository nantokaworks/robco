use std::process::Command;

use crate::model::{AgentNode, ManagementMode, RepoNode};

pub(super) fn repo_named(name: &str) -> RepoNode {
    RepoNode {
        path: format!("/tmp/{name}").into(),
        name: name.to_string(),
        remote_url: None,
        pinned: false,
        management: ManagementMode::Auto,
        agents: Vec::new(),
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

pub(super) fn agent_titled(title: &str, branch: &str) -> AgentNode {
    let now = chrono::Local::now();
    AgentNode {
        management: ManagementMode::Manual,
        id: "agent123".to_string(),
        parent_agent_id: None,
        title: title.to_string(),
        task_number: None,
        worktree_path: "/tmp/wt".into(),
        branch: branch.to_string(),
        base_commit: String::new(),
        program: "claude".to_string(),
        claude_session_id: None,
        profile: None,
        tmux_session: "robco_dropr_t".to_string(),
        created_at: now,
        updated_at: now,
        status: Default::default(),
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

pub(super) fn run_git(cwd: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(["-C", cwd.to_str().unwrap()])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
