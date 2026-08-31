use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::{config::Config, model::RepoNode, registry::Registry};

fn repo(path: &str) -> RepoNode {
    RepoNode {
        path: path.into(),
        name: path.to_string(),
        remote_url: None,
        pinned: false,
        agents: Vec::new(),
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

fn test_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    App::new(Registry::default(), Config::default(), temp.path().into())
}

#[test]
fn a_child_worktree_row_is_refused_and_every_other_selection_passes_through() {
    assert_eq!(
        selection_refusal(Some(Selection::ChildWorktree {
            repo: 0,
            agent: 0,
            child: 0,
        })),
        Some("branch update is not available for child worktrees")
    );
    for selection in [
        None,
        Some(Selection::Repo(0)),
        Some(Selection::Agent { repo: 0, agent: 0 }),
    ] {
        assert_eq!(selection_refusal(selection), None);
    }
}

#[test]
fn nothing_selected_is_a_silent_no_op() {
    let mut app = test_app();
    app.registry.repos = vec![repo("/repo")];
    app.selected = usize::MAX;

    app.update_branch_selected();

    assert!(app.message.is_none());
}

/// Pins the `u` binding itself (`ui::input::handle_key_with_pr_sender`),
/// not just the action it calls: a key that stopped routing here would pass
/// every other test in this file while leaving the operator with no way to
/// reach `update_branch_selected` at all.
#[test]
fn the_u_key_routes_to_the_update_branch_action() {
    let mut app = test_app();
    app.registry.repos = vec![repo("/repo")];
    app.selected = usize::MAX;

    let quit = app
        .handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE))
        .unwrap();

    assert!(!quit);
    assert!(app.message.is_none());
}
