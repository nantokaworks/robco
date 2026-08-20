//! Registry fixtures shared by the sidebar-layout tests.
//!
//! `RepoNode` / `AgentNode` are wide structs with a lot of runtime-only fields,
//! and `Registry` is not `Clone`, so a test that compares two launches has to
//! build the same shape twice. Spelling that out once here keeps the test files
//! about the behaviour they cover.

use std::path::{Path, PathBuf};

use crate::{
    model::{AgentNode, RepoNode, Status},
    registry::Registry,
};

pub(in crate::ui) fn agent(id: &str, worktree_path: PathBuf) -> AgentNode {
    let now = chrono::Local::now();
    AgentNode {
        id: id.to_string(),
        parent_agent_id: None,
        title: id.to_string(),
        task_number: None,
        worktree_path,
        branch: format!("robco/{id}"),
        base_commit: String::new(),
        program: "claude".to_string(),
        claude_session_id: None,
        profile: None,
        tmux_session: format!("robco_{id}"),
        created_at: now,
        updated_at: now,
        status: Status::default(),
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

pub(in crate::ui) fn repo(path: PathBuf, agents: Vec<AgentNode>) -> RepoNode {
    RepoNode {
        name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("repo")
            .to_string(),
        path,
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

/// Agent-less repos directly under `dir`, so they read as local rows of the
/// launch directory rather than landing in the "other locations" section.
///
/// Sorted by name, because that is what discovery hands the registry over —
/// the state a saved order has to beat.
pub(in crate::ui) fn registry_under(dir: &Path, names: &[&str]) -> Registry {
    let mut names = names.to_vec();
    names.sort_unstable();
    Registry {
        version: 1,
        repos: names
            .iter()
            .map(|name| repo(dir.join(name), Vec::new()))
            .collect(),
    }
}

/// `alpha` with one agent whose worktree sits under `dir`.
pub(in crate::ui) fn registry_with_agent(dir: &Path) -> Registry {
    Registry {
        version: 1,
        repos: vec![repo(
            dir.join("alpha"),
            vec![agent("one", dir.join("worktrees/one"))],
        )],
    }
}
