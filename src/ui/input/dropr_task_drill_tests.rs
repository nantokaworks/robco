use crossterm::event::KeyCode;

use super::*;
use crate::{
    config::Config,
    dropr::{DroprTaskCandidate, DroprTaskFetch},
    model::{ManagementMode, RepoNode},
    registry::Registry,
    ui::{DroprTaskFocus, Mode},
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
fn list_focus_claims_movement_and_opens_the_reading_dialog_on_enter() {
    let mut app = app_with_tasks();
    app.dropr_task_focus = Some(DroprTaskFocus { task: 0 });

    assert!(handle_normal(&mut app, KeyCode::Down));
    assert_eq!(app.dropr_task_focus, Some(DroprTaskFocus { task: 1 }));

    // Enter opens `Mode::TaskBody` (dropr:501) — a distinct mode with its own
    // exclusive key routing in `ui::input`, not this guard clause — so the
    // list's own focus is left exactly where it was; closing the dialog is
    // covered by `ui::tests`'s `Mode::TaskBody` coverage, not here.
    assert!(handle_normal(&mut app, KeyCode::Enter));
    assert_eq!(app.dropr_task_focus, Some(DroprTaskFocus { task: 1 }));
    assert!(matches!(app.mode, Mode::TaskBody { task: 1, scroll: 0 }));
}

#[test]
fn list_focus_ignores_unrelated_keys() {
    let mut app = app_with_tasks();
    app.dropr_task_focus = Some(DroprTaskFocus { task: 0 });
    assert!(!handle_normal(&mut app, KeyCode::Char('?')));
}

#[test]
fn list_focus_n_starts_the_launch_path() {
    let mut app = app_with_tasks();
    app.dropr_task_focus = Some(DroprTaskFocus { task: 0 });

    assert!(handle_normal(&mut app, KeyCode::Char('n')));
    // The fixture's task has no dropr workspace linked, so the launch
    // refuses immediately without reaching the network — this only asserts
    // the key is claimed and routed to the same launch path `s` uses from
    // the reading dialog (dropr:482).
    assert!(app.message.is_some());
}

#[test]
fn list_focus_o_starts_the_open_path() {
    let mut app = app_with_tasks();
    app.dropr_task_focus = Some(DroprTaskFocus { task: 0 });

    assert!(handle_normal(&mut app, KeyCode::Char('o')));
    // The fixture's repo has no git remote, so the open action refuses
    // immediately — this only asserts the key is claimed and routed to the
    // browser-open path (dropr:499), not the outcome (covered by
    // `ui::actions::dropr_task_open`'s own tests).
    assert!(app.message.is_some());
}
