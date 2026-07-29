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
fn overseer_category_panes_show_info_only() {
    // The control AI moved to its own row (dropr:370): none of the five
    // categories own a session, so none offers a second tab to cycle to.
    assert_eq!(
        panes_for(Some(Selection::OverseerCategory(OverseerCategory::Health))),
        &[PreviewPane::Info]
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

    // The header is not a row of its own, so the first row is the control AI,
    // followed by the five categories, which are always listed.
    assert_eq!(app.visible().len(), 6);
    assert_eq!(app.selected_item(), Some(Selection::OverseerAi));

    // Inbox is the one category the keys still act on; the read-only three are
    // covered by `overseer_frame::tests::a_leaf_category_cannot_be_expanded_by_any_key`.
    app.selected = OverseerCategory::Inbox.index() + 1;
    assert_eq!(
        app.selected_item(),
        Some(Selection::OverseerCategory(OverseerCategory::Inbox))
    );
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
    // The control AI row (index 0) is the one place `i` opens the prompt now;
    // it has only one preview tab, so unlike before the tab is irrelevant.
    app.selected = 0;
    assert_eq!(app.selected_item(), Some(Selection::OverseerAi));

    app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
        .unwrap();

    assert!(matches!(app.mode, Mode::PromptOverseer { .. }));
}

#[test]
fn expanding_discord_and_selecting_a_channel_row_routes_enter_to_attach() {
    let mut app = test_app();
    app.overseer_visible = true;
    app.orphans = Vec::new();
    let now = chrono::Utc::now();
    app.overseer_snapshot.discord_channels.channels.insert(
        "channel-under-test".into(),
        crate::overseer::discord_channels::ChannelAgent {
            first_seen_at: now,
            last_active_at: now,
            turn_count: 1,
            status: crate::overseer::discord_channels::ChannelAgentStatus::Idle,
            last_error: None,
            history: Vec::new(),
        },
    );

    app.set_overseer_category_expanded(OverseerCategory::Discord, true);
    app.selected = app
        .visible()
        .iter()
        .position(|row| matches!(row, Selection::DiscordChannel(0)))
        .expect("no discord channel row after expanding the category");
    assert_eq!(app.selected_item(), Some(Selection::DiscordChannel(0)));

    // No turn is running for this fabricated channel, so there is no tmux
    // session to attach to. Enter must say so explicitly rather than doing
    // nothing (dropr:371) — not fall through to the generic attach path, and
    // not toggle the category the way an `OverseerCategory` selection would.
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();

    assert!(app.overseer_category_expanded(OverseerCategory::Discord));
    let (message, _) = app.message.expect("Enter on the row must report something");
    assert!(message.contains("no live session"), "{message}");
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
