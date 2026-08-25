use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

use crate::model::{AgentNode, RepoNode};

/// Writes a throwaway executable named `claude` into `dir` that just sleeps,
/// and returns its path. Launch tests need a program whose basename resolves
/// to `claude` — so hook installation (`agent::hooks::write_report_hooks`)
/// and `--session-id` injection still trigger — but that never touches the
/// real CLI and stays alive long enough to pass `tmux::new_worker_session`'s
/// post-launch liveness check (dropr:554); a merely nonexistent path no
/// longer works for this once that check exists, since the pane now dies (and
/// is caught) instead of lingering unobserved.
pub(crate) fn fake_claude_binary(dir: &Path) -> PathBuf {
    let path = dir.join("claude");
    std::fs::write(&path, "#!/bin/sh\nsleep 5\n").unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    path
}

pub(super) fn repo_named(name: &str) -> RepoNode {
    RepoNode {
        path: format!("/tmp/{name}").into(),
        name: name.to_string(),
        remote_url: None,
        pinned: false,
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
        id: "agent123".to_string(),
        parent_agent_id: None,
        title: title.to_string(),
        task_number: None,
        worktree_path: "/tmp/wt".into(),
        branch: branch.to_string(),
        base_commit: String::new(),
        program: "claude".to_string(),
        spawned_by_version: None,
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
