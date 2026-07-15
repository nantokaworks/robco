use chrono::Local;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::{
    config::Config,
    model::{AgentNode, RepoNode, Status},
    registry::Registry,
};

fn agent(id: &str) -> AgentNode {
    let now = Local::now();
    AgentNode {
        id: id.to_string(),
        parent_agent_id: None,
        title: id.to_string(),
        worktree_path: format!("/worktrees/{id}").into(),
        branch: format!("feature/{id}"),
        base_commit: String::new(),
        program: "codex".to_string(),
        profile: None,
        tmux_session: format!("robco_{id}"),
        created_at: now,
        updated_at: now,
        status: Status::Running,
        worktree_missing: false,
        merge_error: None,
        last_capture: None,
        last_change_at: None,
        last_auto_accept_at: None,
        shell_working: false,
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
        dropr_tasks: Vec::new(),
        main_status: None,
        main_last_capture: None,
        main_last_change_at: None,
        main_shell_working: false,
        main_pane_pid: None,
        main_tracked_command: None,
        main_subagents_active: 0,
    }
}

fn test_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    App::new(Registry::default(), Config::default(), temp.path().into())
}

#[test]
fn progress_labels_are_stable_and_specific() {
    assert_eq!(
        [MERGING_PR, PULLING_MAIN, CLEANING_UP],
        ["merging PR", "pulling main", "cleaning up",]
    );
}

#[test]
fn modal_keys_are_swallowed_and_completion_dismisses_once() {
    let mut app = test_app();
    app.mode = Mode::MergeInProgress {
        repo_path: "/repo".into(),
        agent_id: "wanted".into(),
        branch: "feature/wanted".into(),
        step: MERGING_PR,
    };

    let quit = app
        .handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
        .unwrap();
    assert!(!quit);
    assert!(matches!(app.mode, Mode::MergeInProgress { .. }));

    app.mode = Mode::MergeComplete {
        branch: "feature/wanted".into(),
    };
    let quit = app
        .handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
        .unwrap();
    assert!(!quit);
    assert!(matches!(app.mode, Mode::Normal));
}

#[test]
fn failure_resolves_agent_identity_after_index_drift() {
    let mut app = test_app();
    app.registry.repos = vec![
        repo("/repo-a", vec![agent("other")]),
        repo("/repo-b", vec![agent("first"), agent("wanted")]),
    ];
    app.registry.repos.swap(0, 1);
    app.registry.repos[0].agents.swap(0, 1);
    app.mode = Mode::MergeInProgress {
        repo_path: "/repo-b".into(),
        agent_id: "wanted".into(),
        branch: "feature/wanted".into(),
        step: CLEANING_UP,
    };

    app.finish_merge(Err("merge failed".into())).unwrap();

    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(app.registry.repos[0].agents[0].id, "wanted");
    assert_eq!(
        app.registry.repos[0].agents[0].merge_error.as_deref(),
        Some("merge failed")
    );
    assert!(app.registry.repos[0].agents[1].merge_error.is_none());
}

#[test]
fn disconnected_worker_uses_merge_error_path() {
    let mut app = test_app();
    app.registry.repos = vec![repo("/repo", vec![agent("wanted")])];
    app.mode = Mode::MergeInProgress {
        repo_path: "/repo".into(),
        agent_id: "wanted".into(),
        branch: "feature/wanted".into(),
        step: MERGING_PR,
    };
    let (sender, receiver) = mpsc::channel();
    app.merge_receiver = Some(receiver);
    drop(sender);

    app.drain_merge_events().unwrap();

    assert!(matches!(app.mode, Mode::Normal));
    assert!(app.merge_receiver.is_none());
    assert_eq!(
        app.registry.repos[0].agents[0].merge_error.as_deref(),
        Some(WORKER_TERMINATED)
    );
    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some(WORKER_TERMINATED)
    );
}
