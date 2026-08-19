use super::*;
use crate::{
    dropr::{DroprTaskCandidate, DroprTaskFetch},
    model::ManagementMode,
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

#[test]
fn push_rows_adds_one_selection_per_selectable_task() {
    let repo = repo_node(vec![task("#1"), task("#2")]);
    let mut visible = Vec::new();

    push_rows(&mut visible, 3, &repo);

    assert_eq!(
        visible,
        vec![
            Selection::DroprTask { repo: 3, task: 0 },
            Selection::DroprTask { repo: 3, task: 1 },
        ]
    );
}

#[test]
fn item_key_names_the_tasks_own_display_id() {
    let repos = vec![repo_node(vec![task("#42")])];

    assert_eq!(item_key(&repos, 0, 0), "dropr-task:/repo:#42");
}

#[test]
fn item_key_falls_back_to_missing_for_a_stale_index() {
    let repos = vec![repo_node(vec![task("#42")])];

    assert_eq!(item_key(&repos, 0, 5), "dropr-task:/repo:missing");
}
