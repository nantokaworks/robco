use super::*;
use crate::{
    dropr::{DroprTaskCandidate, DroprTaskFetch},
    locale::Locale,
    model::ManagementMode,
};

fn task(display_id: &str) -> DroprTaskCandidate {
    DroprTaskCandidate {
        display_id: display_id.to_string(),
        title: format!("Task {display_id}"),
        description: Some("line one\nline two".to_string()),
        priority: "high".to_string(),
        status: "open".to_string(),
        priority_score: None,
        blocked_reason: None,
        updated_at: None,
        id: format!("id-{display_id}"),
        parent_task_id: None,
        child_count: 0,
    }
}

fn repo_node(
    tasks: Vec<DroprTaskCandidate>,
    subtrees_known: std::collections::HashSet<String>,
) -> RepoNode {
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
            subtrees_known,
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
fn renders_title_status_priority_and_description() {
    let repo = repo_node(vec![task("#1")], Default::default());
    let (title, text) = render(&repo, 0, Locale::En).expect("task at index 0");
    assert_eq!(title, "repo / #1");
    let rendered = text.to_string();
    assert!(rendered.contains("Task #1"));
    assert!(rendered.contains("status: open"));
    assert!(rendered.contains("priority: high"));
    assert!(rendered.contains("line one"));
    assert!(rendered.contains("line two"));
}

#[test]
fn a_missing_description_says_so_instead_of_a_blank_body() {
    let mut candidate = task("#1");
    candidate.description = None;
    let repo = repo_node(vec![candidate], Default::default());
    let (_, text) = render(&repo, 0, Locale::En).expect("task at index 0");
    assert!(text.to_string().contains("(no description)"));
}

#[test]
fn a_stale_index_renders_nothing() {
    let repo = repo_node(vec![task("#1")], Default::default());
    assert!(render(&repo, 5, Locale::En).is_none());
}

#[test]
fn a_childless_task_says_it_has_no_subtasks() {
    let repo = repo_node(vec![task("#1")], Default::default());
    let (_, text) = render(&repo, 0, Locale::En).expect("task at index 0");
    assert!(text.to_string().contains("(no subtasks)"));
}

#[test]
fn a_parent_whose_subtree_was_not_fetched_says_so() {
    let mut parent = task("#1");
    parent.child_count = 2;
    let repo = repo_node(vec![parent], Default::default());
    let (_, text) = render(&repo, 0, Locale::En).expect("task at index 0");
    assert!(text.to_string().contains("subtasks were not fetched"));
}

#[test]
fn a_fetched_subtree_lists_its_children() {
    let mut parent = task("#1");
    parent.child_count = 1;
    let mut child = task("#2");
    child.parent_task_id = Some(parent.id.clone());
    let subtrees_known = [parent.id.clone()].into_iter().collect();
    let repo = repo_node(vec![parent, child], subtrees_known);
    let (_, text) = render(&repo, 0, Locale::En).expect("task at index 0");
    assert!(text.to_string().contains("#2"));
}
