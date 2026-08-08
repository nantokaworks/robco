use std::path::PathBuf;

use super::*;
use crate::model::{ManagementMode, Status};

fn repo(path: &str, agents: Vec<AgentNode>) -> RepoNode {
    RepoNode {
        path: PathBuf::from(path),
        name: path.rsplit('/').next().unwrap_or("repo").to_string(),
        remote_url: None,
        pinned: false,
        management: crate::model::ManagementMode::Auto,
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

fn agent(id: &str) -> AgentNode {
    let now = chrono::Local::now();
    AgentNode {
        id: id.to_string(),
        parent_agent_id: None,
        management: ManagementMode::Manual,
        title: id.to_string(),
        task_number: None,
        worktree_path: PathBuf::from(format!("/tmp/{id}")),
        branch: id.to_string(),
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

/// A reload drops every `#[serde(skip)]` field, so without the carry the status
/// column would blank out on every registry write until the next refresh tick.
#[test]
fn runtime_fields_survive_a_reload() {
    let mut source = repo("/a/one", vec![agent("worker")]);
    source.main_status = Some(Status::Running);
    source.main_subagents_active = 3;
    source.agents[0].status = Status::Running;
    source.agents[0].pane_pid = Some(4242);
    source.agents[0].merge_error = Some("gh failed".into());
    source.agents[0].children.push(crate::model::ChildWorktree {
        path: "/tmp/worker/child".into(),
        branch: Some("child".into()),
        head: None,
        clean: None,
        ahead_behind: None,
        tmux_session: None,
        modified_at: None,
    });

    let mut reloaded = vec![repo("/a/one", vec![agent("worker")])];
    carry_runtime(std::slice::from_ref(&source), &mut reloaded);

    assert_eq!(reloaded[0].main_status, Some(Status::Running));
    assert_eq!(reloaded[0].main_subagents_active, 3);
    assert_eq!(reloaded[0].agents[0].status, Status::Running);
    assert_eq!(reloaded[0].agents[0].pane_pid, Some(4242));
    assert_eq!(
        reloaded[0].agents[0].merge_error.as_deref(),
        Some("gh failed")
    );
    assert_eq!(reloaded[0].agents[0].children.len(), 1);
}

/// The whole point of routing through `locked_update`: a worker another process
/// committed comes back from the reload, and the carry must leave it in place
/// rather than treat its missing runtime state as a reason to drop it.
#[test]
fn rows_only_present_on_disk_are_kept_with_default_runtime_state() {
    let current = vec![repo("/a/one", vec![agent("mine")])];
    let mut reloaded = vec![repo("/a/one", vec![agent("mine"), agent("theirs")])];

    carry_runtime(&current, &mut reloaded);

    let ids: Vec<&str> = reloaded[0]
        .agents
        .iter()
        .map(|agent| agent.id.as_str())
        .collect();
    assert_eq!(ids, ["mine", "theirs"]);
    assert_eq!(reloaded[0].agents[1].pane_pid, None);
}

#[test]
fn expanded_flags_follow_their_repo_across_an_insert() {
    let current = vec![repo("/a/one", Vec::new()), repo("/a/three", Vec::new())];
    let reloaded = vec![
        repo("/a/one", Vec::new()),
        repo("/a/two", Vec::new()),
        repo("/a/three", Vec::new()),
    ];

    // A repo added by another process defaults to expanded; the collapsed flag
    // stays on `/a/three` instead of shifting onto the newcomer.
    assert_eq!(
        realign_expanded(&current, &[true, false], &reloaded),
        [true, true, false]
    );
}

#[test]
fn expanded_flags_drop_with_a_removed_repo() {
    let current = vec![repo("/a/one", Vec::new()), repo("/a/two", Vec::new())];
    let reloaded = vec![repo("/a/two", Vec::new())];

    assert_eq!(
        realign_expanded(&current, &[false, true], &reloaded),
        [true]
    );
}
