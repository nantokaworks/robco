use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::{config::Config, model::OverseerCategory, registry::Registry};

fn test_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    App::new(Registry::default(), Config::default(), temp.path().into())
}

#[test]
fn prompt_agent_inserts_at_the_cursor_instead_of_appending() {
    let mut app = test_app();
    app.mode = Mode::PromptAgent {
        repo: 0,
        input: TextInput::from("wrker"),
    };

    for code in [KeyCode::Home, KeyCode::Right, KeyCode::Char('o')] {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
            .unwrap();
    }

    assert!(matches!(&app.mode, Mode::PromptAgent { input, .. } if input == "worker"));
}

#[test]
fn prompt_repo_word_deletion_reaches_the_shared_buffer() {
    let mut app = test_app();
    app.mode = Mode::PromptRepo {
        input: TextInput::from("https://example.com/repo.git main"),
    };

    app.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL))
        .unwrap();

    assert!(
        matches!(&app.mode, Mode::PromptRepo { input } if input == "https://example.com/repo.git ")
    );
}

#[test]
fn overseer_category_panes_show_info_then_claude() {
    assert_eq!(
        panes_for(Some(Selection::OverseerCategory(OverseerCategory::Health))),
        &[PreviewPane::Info, PreviewPane::Claude]
    );
}

#[test]
fn overseer_expand_collapse_keys_update_tree() {
    let mut app = test_app();
    app.overseer_visible = true;
    app.selected = 0;
    // Ignore any live robco tmux sessions the host discovers as orphans so the
    // tree contents are deterministic across environments.
    app.orphans = Vec::new();

    // The header is not a row of its own, so the first row is already a
    // category and the four categories are always listed.
    assert_eq!(app.visible().len(), 4);
    assert_eq!(
        app.selected_item(),
        Some(Selection::OverseerCategory(OverseerCategory::Health))
    );

    // Inbox is the one category the keys still act on; the read-only three are
    // covered by `overseer_frame::tests::a_leaf_category_cannot_be_expanded_by_any_key`.
    app.selected = OverseerCategory::Inbox.index();
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
    assert!(app.overseer_category_expanded(OverseerCategory::Inbox));
    app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
        .unwrap();
    assert!(!app.overseer_category_expanded(OverseerCategory::Inbox));
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
        .unwrap();
    assert!(app.overseer_category_expanded(OverseerCategory::Inbox));
}

#[test]
fn overseer_instruction_key_opens_prompt() {
    let mut app = test_app();
    app.overseer_visible = true;
    app.selected = 0;
    app.preview = PreviewPane::Claude;

    app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
        .unwrap();

    assert!(matches!(app.mode, Mode::PromptOverseer { .. }));
}

#[test]
fn visible_message_does_not_swallow_next_key() {
    let mut app = test_app();
    app.show_message("done");
    let quit = app
        .handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE))
        .unwrap();

    assert!(!quit);
    assert!(app.message.is_none());
    assert!(matches!(app.mode, Mode::Help { scroll: 0 }));
}

#[test]
fn confirm_pr_y_and_n_edit_and_escape_cancels() {
    let mut app = test_app();
    app.mode = Mode::ConfirmPr {
        repo_path: "/repo".into(),
        agent_id: "agent".to_string(),
        branch: "feature/agent".to_string(),
        input: "prompt".into(),
    };

    for code in [KeyCode::Char('y'), KeyCode::Char('n'), KeyCode::Backspace] {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
            .unwrap();
    }
    assert!(matches!(
        &app.mode,
        Mode::ConfirmPr { input, .. } if input == "prompty"
    ));

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    assert!(matches!(app.mode, Mode::Normal));
}
