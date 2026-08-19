use crossterm::event::KeyCode;

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

fn app_with_tasks() -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.registry.repos = vec![repo_node(vec![task("#1"), task("#2")])];
    // The fixture's repo path is not under the app's launch dir, so it
    // renders under "other locations" rather than at index 0.
    app.selected = app
        .visible()
        .iter()
        .position(|selection| matches!(selection, crate::model::Selection::Repo(0)))
        .expect("repo row is visible");
    app
}

#[test]
fn unfocused_claims_nothing() {
    let mut app = app_with_tasks();
    assert!(!handle_normal(&mut app, KeyCode::Down));
    assert!(!handle_normal(&mut app, KeyCode::Enter));
}

#[test]
fn list_focus_claims_movement_open_and_back() {
    let mut app = app_with_tasks();
    app.dropr_task_focus = Some(DroprTaskFocus::List { task: 0 });

    assert!(handle_normal(&mut app, KeyCode::Down));
    assert_eq!(app.dropr_task_focus, Some(DroprTaskFocus::List { task: 1 }));

    assert!(handle_normal(&mut app, KeyCode::Enter));
    assert_eq!(app.dropr_task_focus, Some(DroprTaskFocus::Body { task: 1 }));

    assert!(handle_normal(&mut app, KeyCode::Esc));
    assert_eq!(app.dropr_task_focus, Some(DroprTaskFocus::List { task: 1 }));
}

#[test]
fn list_focus_ignores_unrelated_keys() {
    let mut app = app_with_tasks();
    app.dropr_task_focus = Some(DroprTaskFocus::List { task: 0 });
    assert!(!handle_normal(&mut app, KeyCode::Char('n')));
    assert!(!handle_normal(&mut app, KeyCode::Char('?')));
}

#[test]
fn body_focus_claims_scroll_and_back_but_not_launch_on_enter() {
    let mut app = app_with_tasks();
    app.dropr_task_focus = Some(DroprTaskFocus::Body { task: 1 });

    assert!(!handle_normal(&mut app, KeyCode::Enter));
    assert!(handle_normal(&mut app, KeyCode::Down));
    assert!(handle_normal(&mut app, KeyCode::PageUp));

    assert!(handle_normal(&mut app, KeyCode::Char('h')));
    assert_eq!(app.dropr_task_focus, Some(DroprTaskFocus::List { task: 1 }));
}

#[test]
fn body_focus_s_starts_the_launch_path() {
    let mut app = app_with_tasks();
    app.dropr_task_focus = Some(DroprTaskFocus::Body { task: 0 });

    assert!(handle_normal(&mut app, KeyCode::Char('s')));
    // The listed task has no dropr workspace linked, so the launch refuses
    // immediately without reaching the network — this only asserts the key
    // is claimed and routed, not the launch outcome (covered by
    // `ui::actions::dropr_task_drill`'s own tests).
    assert!(app.message.is_some());
}
