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
        description: None,
        priority: String::new(),
        status: "open".to_string(),
        priority_score: None,
        blocked_reason: None,
        updated_at: None,
        id: format!("id-{display_id}"),
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

/// Points `app.selected` at `Selection::Repo(0)`, wherever `App::visible`
/// actually placed it — the fixture's repo path is not under the app's
/// launch dir, so it renders under "other locations" rather than at index 0.
fn select_repo_row(app: &mut App) {
    app.selected = app
        .visible()
        .iter()
        .position(|selection| matches!(selection, Selection::Repo(0)))
        .expect("repo row is visible");
}

#[test]
fn entering_the_task_list_starts_on_the_first_row() {
    let mut app = test_app();
    app.enter_dropr_task_list();
    assert_eq!(app.dropr_task_focus, Some(DroprTaskFocus::List { task: 0 }));
}

#[test]
fn leaving_the_task_list_clears_focus() {
    let mut app = test_app();
    app.dropr_task_focus = Some(DroprTaskFocus::List { task: 2 });
    app.leave_dropr_task_list();
    assert_eq!(app.dropr_task_focus, None);
}

#[test]
fn moving_the_task_cursor_clamps_to_what_is_listed() {
    let mut app = test_app();
    app.registry.repos = vec![repo_node(vec![task("#1"), task("#2")])];
    select_repo_row(&mut app);
    app.dropr_task_focus = Some(DroprTaskFocus::List { task: 0 });

    app.move_dropr_task_cursor(-1);
    assert_eq!(app.dropr_task_focus, Some(DroprTaskFocus::List { task: 0 }));

    app.move_dropr_task_cursor(1);
    assert_eq!(app.dropr_task_focus, Some(DroprTaskFocus::List { task: 1 }));

    app.move_dropr_task_cursor(1);
    assert_eq!(app.dropr_task_focus, Some(DroprTaskFocus::List { task: 1 }));
}

#[test]
fn opening_a_stale_task_row_does_nothing() {
    let mut app = test_app();
    app.registry.repos = vec![repo_node(vec![task("#1")])];
    select_repo_row(&mut app);
    app.dropr_task_focus = Some(DroprTaskFocus::List { task: 5 });

    app.open_dropr_task_body();

    assert_eq!(app.dropr_task_focus, Some(DroprTaskFocus::List { task: 5 }));
}

#[test]
fn opening_a_listed_task_row_moves_to_its_body() {
    let mut app = test_app();
    app.registry.repos = vec![repo_node(vec![task("#1")])];
    select_repo_row(&mut app);
    app.dropr_task_focus = Some(DroprTaskFocus::List { task: 0 });

    app.open_dropr_task_body();

    assert_eq!(app.dropr_task_focus, Some(DroprTaskFocus::Body { task: 0 }));
}

#[test]
fn closing_the_body_returns_to_the_list_on_the_same_task() {
    let mut app = test_app();
    app.dropr_task_focus = Some(DroprTaskFocus::Body { task: 3 });
    app.close_dropr_task_body();
    assert_eq!(app.dropr_task_focus, Some(DroprTaskFocus::List { task: 3 }));
}
