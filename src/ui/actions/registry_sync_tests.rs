use std::path::{Path, PathBuf};

use super::{adopt_stored_agents, dialog_agent, restore_dialog_agent};
use crate::{
    model::{AgentNode, ManagementMode, RepoNode, Status},
    registry::Registry,
    ui::Mode,
};

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
    }
}

fn agent(id: &str) -> AgentNode {
    let now = chrono::Local::now();
    AgentNode {
        id: id.to_string(),
        parent_agent_id: None,
        management: ManagementMode::Auto,
        title: id.to_string(),
        task_number: None,
        worktree_path: PathBuf::from(format!("/wt/{id}")),
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

fn stored(repos: Vec<RepoNode>) -> Registry {
    Registry { version: 1, repos }
}

fn ids(repo: &RepoNode) -> Vec<&str> {
    repo.agents.iter().map(|agent| agent.id.as_str()).collect()
}

/// The reported defect: the Overseer's post-merge cleanup drops the agent from
/// `state.json`, and the TUI keeps rendering — and attaching to — a row whose
/// worktree, branch, and session are gone.
#[test]
fn a_row_another_process_removed_is_dropped() {
    let mut repos = vec![repo("/a/one", vec![agent("kept"), agent("cleaned-up")])];
    let stored = stored(vec![repo("/a/one", vec![agent("kept")])]);

    adopt_stored_agents(&mut repos, &stored);

    assert_eq!(ids(&repos[0]), ["kept"]);
}

/// The dropped row's `merge_error` is runtime-only and never reaches disk, so
/// nothing else would clear it. Leaving it behind would keep the row's
/// `merge-failed` badge alive in the per-repo count after the row itself is gone.
#[test]
fn a_dropped_rows_merge_error_does_not_survive_it() {
    let mut repos = vec![repo("/a/one", vec![agent("kept"), agent("cleaned-up")])];
    repos[0].agents[0].merge_error = Some("gh failed".into());
    repos[0].agents[1].merge_error = Some("merge race".into());
    let stored = stored(vec![repo("/a/one", vec![agent("kept")])]);

    adopt_stored_agents(&mut repos, &stored);

    let failed = repos[0]
        .agents
        .iter()
        .filter(|agent| agent.merge_error.is_some())
        .count();
    assert_eq!(failed, 1);
    assert_eq!(repos[0].agents[0].merge_error.as_deref(), Some("gh failed"));
}

/// The stored row is parsed fresh, so every `#[serde(skip)]` field on it is at
/// its default. Without the carry, a reconcile tick would blank the status
/// column and the badges of every row that did survive.
#[test]
fn a_surviving_row_keeps_its_runtime_state() {
    let mut repos = vec![repo("/a/one", vec![agent("kept")])];
    let live = &mut repos[0].agents[0];
    live.status = Status::Running;
    live.pane_pid = Some(4242);
    live.merge_error = Some("gh failed".into());
    live.last_capture = Some("working".into());
    live.children.push(crate::model::ChildWorktree {
        path: "/wt/kept/child".into(),
        branch: Some("child".into()),
        head: None,
        clean: None,
        ahead_behind: None,
        tmux_session: None,
        modified_at: None,
    });
    let stored = stored(vec![repo("/a/one", vec![agent("kept")])]);

    adopt_stored_agents(&mut repos, &stored);

    let kept = &repos[0].agents[0];
    assert_eq!(kept.status, Status::Running);
    assert_eq!(kept.pane_pid, Some(4242));
    assert_eq!(kept.merge_error.as_deref(), Some("gh failed"));
    assert_eq!(kept.last_capture.as_deref(), Some("working"));
    assert_eq!(kept.children.len(), 1);
}

/// Membership runs both ways: a worker the daemon spawned while the TUI was up
/// should appear without waiting for this process to write the registry.
#[test]
fn a_row_this_process_has_never_seen_is_adopted() {
    let mut repos = vec![repo("/a/one", vec![agent("mine")])];
    let stored = stored(vec![repo("/a/one", vec![agent("mine"), agent("theirs")])]);

    adopt_stored_agents(&mut repos, &stored);

    assert_eq!(ids(&repos[0]), ["mine", "theirs"]);
    assert_eq!(repos[0].agents[1].pane_pid, None);
}

/// Persisted edits belong to disk too: a management mode another client changed
/// must not be overwritten by this process's stale copy of the same row.
#[test]
fn a_surviving_row_takes_the_stored_persisted_fields() {
    let mut repos = vec![repo("/a/one", vec![agent("kept")])];
    repos[0].agents[0].management = ManagementMode::Auto;
    let mut stored_repo = repo("/a/one", vec![agent("kept")]);
    stored_repo.agents[0].management = ManagementMode::Manual;

    adopt_stored_agents(&mut repos, &stored(vec![stored_repo]));

    assert_eq!(repos[0].agents[0].management, ManagementMode::Manual);
}

/// Discovery adopts a repository into memory a tick before the save thread
/// persists it. Treating "not on disk" as "removed" there would empty a repo
/// that was only ever unwritten — and, next tick, adopt every worktree again.
#[test]
fn a_repo_the_registry_has_not_stored_keeps_its_agents() {
    let mut repos = vec![repo("/a/one", vec![agent("adopted")])];

    adopt_stored_agents(&mut repos, &stored(vec![repo("/b/two", Vec::new())]));

    assert_eq!(ids(&repos[0]), ["adopted"]);
}

/// The operator opens `m` on a worker the Overseer is already cleaning up. The
/// dialog's stored index would then address the row that slid into the slot —
/// or, at the end of the list, panic the draw path that indexes with it.
#[test]
fn a_dialog_follows_its_agent_across_a_dropped_row() {
    let before = vec![repo("/a/one", vec![agent("cleaned-up"), agent("kept")])];
    let mut mode = Mode::ConfirmMerge { repo: 0, agent: 1 };

    let anchor = dialog_agent(&mode, &before);
    let after = vec![repo("/a/one", vec![agent("kept")])];
    restore_dialog_agent(&mut mode, &after, anchor);

    assert!(matches!(mode, Mode::ConfirmMerge { repo: 0, agent: 0 }));
}

#[test]
fn a_dialog_whose_agent_is_gone_closes() {
    let before = vec![repo("/a/one", vec![agent("kept"), agent("cleaned-up")])];
    let mut mode = Mode::ConfirmCleanup { repo: 0, agent: 1 };

    let anchor = dialog_agent(&mode, &before);
    let after = vec![repo("/a/one", vec![agent("kept")])];
    restore_dialog_agent(&mut mode, &after, anchor);

    assert!(matches!(mode, Mode::Normal));
}

/// A dialog that addresses nothing even before the swap — its indices already
/// out of range — must close rather than survive into the next draw.
#[test]
fn a_dialog_with_unresolvable_indices_closes() {
    let mut mode = Mode::ConfirmKill { repo: 4, agent: 0 };

    let anchor = dialog_agent(&mode, &[]);
    assert!(anchor.is_none());

    restore_dialog_agent(&mut mode, &[repo("/a/one", vec![agent("kept")])], anchor);

    assert!(matches!(mode, Mode::Normal));
}

/// Dialogs that never stored an index — and plain `Mode::Normal` — are left
/// alone; closing them on a refresh tick would cancel the operator's typing.
#[test]
fn a_dialog_without_agent_indices_is_untouched() {
    let repos = vec![repo("/a/one", vec![agent("kept")])];
    let mut mode = Mode::ConfirmRemoveRepo {
        path: "/a/one".into(),
    };

    let anchor = dialog_agent(&mode, &repos);
    restore_dialog_agent(&mut mode, &repos, anchor);

    assert!(matches!(mode, Mode::ConfirmRemoveRepo { path } if path == Path::new("/a/one")));
}
