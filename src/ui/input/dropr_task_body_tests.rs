use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::{
    config::Config,
    dropr::{DroprTaskCandidate, DroprTaskFetch},
    model::{RepoNode, Selection},
    registry::Registry,
    ui::DroprTaskFocus,
};

/// A repo with one task, no linked dropr workspace, and no git remote — `s`
/// and `o` refuse immediately without reaching the network, so these tests
/// only prove the key is routed to the right path (dropr:501).
fn app_with_a_task_reading() -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.registry.repos = vec![RepoNode {
        path: "/repo".into(),
        name: "repo".into(),
        remote_url: None,
        pinned: true,
        agents: Vec::new(),
        dropr: None,
        dropr_tasks: DroprTaskFetch {
            tasks: vec![DroprTaskCandidate {
                display_id: "#1".to_string(),
                title: "Task #1".to_string(),
                description: None,
                priority: String::new(),
                status: "open".to_string(),
                priority_score: None,
                blocked_reason: None,
                updated_at: None,
                id: "id-1".to_string(),
                parent_task_id: None,
                child_count: 0,
            }],
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
    }];
    app.selected = app
        .visible()
        .iter()
        .position(|selection| matches!(selection, Selection::Repo(0)))
        .expect("repo row is visible");
    app.dropr_task_focus = Some(DroprTaskFocus { task: 0 });
    app.mode = Mode::TaskBody { task: 0, scroll: 3 };
    app
}

#[test]
fn task_body_scroll_keys_adjust_the_dialogs_own_scroll() {
    let mut app = app_with_a_task_reading();

    for code in [KeyCode::Down, KeyCode::Char('j')] {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
            .unwrap();
    }
    assert!(matches!(app.mode, Mode::TaskBody { task: 0, scroll: 5 }));

    app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE))
        .unwrap();
    assert!(matches!(
        app.mode,
        Mode::TaskBody {
            task: 0,
            scroll: 15
        }
    ));

    app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE))
        .unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE))
        .unwrap();
    assert!(matches!(app.mode, Mode::TaskBody { task: 0, scroll: 4 }));
}

#[test]
fn task_body_esc_returns_to_normal_without_moving_the_list() {
    let mut app = app_with_a_task_reading();

    for code in [KeyCode::Esc, KeyCode::Left, KeyCode::Char('h')] {
        app.mode = Mode::TaskBody { task: 0, scroll: 3 };
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
            .unwrap();
        assert!(matches!(app.mode, Mode::Normal));
        // The list cursor stayed on the same row the whole time — closing
        // the dialog never touches `dropr_task_focus` (dropr:501).
        assert_eq!(app.dropr_task_focus, Some(DroprTaskFocus { task: 0 }));
    }
}

#[test]
fn task_body_s_routes_to_the_launch_path() {
    let mut app = app_with_a_task_reading();

    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE))
        .unwrap();

    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("no dropr workspace linked to this repo")
    );
}

#[test]
fn task_body_o_routes_to_the_open_path() {
    let mut app = app_with_a_task_reading();

    app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE))
        .unwrap();

    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("this repository has no git remote")
    );
}

#[test]
fn task_body_swallows_quit_and_help_while_open() {
    let mut app = app_with_a_task_reading();

    let quit = app
        .handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
        .unwrap();
    assert!(!quit);
    assert!(matches!(app.mode, Mode::TaskBody { .. }));

    app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE))
        .unwrap();
    assert!(matches!(app.mode, Mode::TaskBody { .. }));
}
