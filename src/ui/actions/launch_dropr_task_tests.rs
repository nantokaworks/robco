use super::*;
use crate::{
    config::Config,
    dropr::{DroprTaskCandidate, DroprTaskFetch},
    model::{ManagementMode, RepoNode},
    registry::Registry,
};

fn task(display_id: &str) -> DroprTaskCandidate {
    DroprTaskCandidate {
        display_id: display_id.to_string(),
        title: format!("Task {display_id}"),
        priority: String::new(),
        status: "open".to_string(),
        priority_score: None,
        blocked_reason: None,
        updated_at: None,
        id: String::new(),
        parent_task_id: None,
        child_count: 0,
    }
}

fn repo_node(tasks: Vec<DroprTaskCandidate>) -> RepoNode {
    RepoNode {
        path: "/repo".into(),
        name: "repo".into(),
        remote_url: None,
        pinned: true,
        management: ManagementMode::Auto,
        agents: Vec::new(),
        dropr: None,
        dropr_tasks: DroprTaskFetch {
            tasks,
            problems: Vec::new(),
            answered: true,
            subtrees_known: Default::default(),
        },
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

fn test_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    App::new(Registry::default(), Config::default(), temp.path().into())
}

#[test]
fn a_repo_index_gone_since_selection_only_shows_a_message() {
    let mut app = test_app();
    app.registry.repos = vec![repo_node(vec![task("#1")])];

    app.launch_dropr_task_selected(1, 0);

    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("repository changed, nothing to launch")
    );
}

#[test]
fn a_task_index_no_longer_listed_only_shows_a_message() {
    let mut app = test_app();
    app.registry.repos = vec![repo_node(vec![task("#1")])];

    app.launch_dropr_task_selected(0, 5);

    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("task is no longer listed")
    );
}
