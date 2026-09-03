use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::{
    config::Config,
    model::{OverseerCategory, Selection},
    registry::Registry,
    ui::text_input::TextInput,
};

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
    app.orphans = Vec::new();

    // The header is not a row of its own, so the first row is the control AI,
    // followed by the five categories, which are always listed.
    assert_eq!(app.visible().len(), 6);
    assert_eq!(app.selected_item(), Some(Selection::OverseerAi));

    // Inbox is the one category these keys are asserted on; read-only Health is
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

/// An app with one repo and one running agent, agent row selected.
fn test_app_with_agent() -> App {
    let temp = tempfile::tempdir().unwrap();
    let config = Config {
        worktree_root: temp.path().join("worktrees"),
        ..Config::default()
    };
    let mut app = App::new(
        test_support::registry_with_agent(temp.path()),
        config,
        temp.path().into(),
    );
    app.selected = app
        .visible()
        .iter()
        .position(|row| matches!(row, Selection::Agent { .. }))
        .expect("no agent row");
    app
}

#[test]
fn i_on_the_claude_tab_opens_the_instruct_session_prompt() {
    let mut app = test_app_with_agent();
    app.preview = PreviewPane::Claude;

    app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
        .unwrap();

    assert!(matches!(&app.mode, Mode::PromptSession { session, .. } if session == "robco_one"));
}

#[test]
fn esc_cancels_the_instruct_session_prompt_without_sending() {
    let mut app = test_app_with_agent();
    app.mode = Mode::PromptSession {
        session: "robco_one".into(),
        host: None,
        input: TextInput::from("hello"),
    };

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();

    assert!(matches!(app.mode, Mode::Normal));
}

#[test]
fn i_on_a_non_claude_tab_does_not_open_the_instruct_session_prompt() {
    let mut app = test_app_with_agent();
    app.preview = PreviewPane::Info;

    app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
        .unwrap();

    assert!(matches!(app.mode, Mode::Normal));
}

#[test]
fn i_with_no_live_session_shows_a_message_instead_of_opening() {
    let mut app = test_app_with_agent();
    app.preview = PreviewPane::Claude;
    let Some(Selection::Agent { repo, agent }) = app.selected_item() else {
        panic!("no agent row selected");
    };
    app.registry.repos[repo].agents[agent].status = crate::model::Status::BranchOnly;

    app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
        .unwrap();

    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("no live session for this tab")
    );
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
            channel_name: None,
        },
    );

    app.set_overseer_category_expanded(OverseerCategory::Discord, true);
    app.selected = app
        .visible()
        .iter()
        .position(|row| matches!(row, Selection::DiscordChannel(0)))
        .expect("no discord channel row after expanding the category");
    assert_eq!(app.selected_item(), Some(Selection::DiscordChannel(0)));

    // Enter must route to the channel's own attach action rather than the
    // category toggle a bare `Selection::OverseerCategory` selection would
    // take (dropr:371) — proven here by staying expanded and showing some
    // explicit response rather than doing nothing. The exact wording of that
    // response depends on `tmux` actually being on PATH (present locally and
    // on the `ubuntu-latest` CI runner, absent on `macos-latest`), so it is
    // asserted deterministically, without a live tmux dependency, by
    // `a_channel_no_longer_listed_reports_explicitly_instead_of_doing_nothing`
    // below via the out-of-range index branch, which never reaches `tmux`.
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();

    assert!(app.overseer_category_expanded(OverseerCategory::Discord));
    assert!(
        app.message.is_some(),
        "Enter on the row must report something rather than doing nothing silently"
    );
}

#[test]
fn a_channel_no_longer_listed_reports_explicitly_instead_of_doing_nothing() {
    // Deterministic, tmux-free coverage of the "say so explicitly" contract
    // (dropr:371): selecting a channel row whose index has since fallen out
    // of range (the retained-channel list shrank between render and Enter)
    // must not attempt to derive or attach a session at all — it reports and
    // returns before ever calling `tmux`.
    let mut app = test_app();
    app.overseer_visible = true;
    app.orphans = Vec::new();
    app.selected = 0;

    app.attach_discord_channel_selected(0);
    assert!(app.message.is_none(), "not selected on this row: no-op");

    app.overseer_snapshot.discord_channels.channels.insert(
        "only-channel".into(),
        crate::overseer::discord_channels::ChannelAgent {
            first_seen_at: chrono::Utc::now(),
            last_active_at: chrono::Utc::now(),
            turn_count: 0,
            status: crate::overseer::discord_channels::ChannelAgentStatus::Idle,
            last_error: None,
            history: Vec::new(),
            channel_name: None,
        },
    );
    app.set_overseer_category_expanded(OverseerCategory::Discord, true);
    app.selected = app
        .visible()
        .iter()
        .position(|row| matches!(row, Selection::DiscordChannel(0)))
        .expect("no discord channel row after expanding the category");

    app.attach_discord_channel_selected(1);
    let (message, _) = app
        .message
        .expect("an out-of-range channel index must report explicitly");
    assert!(message.contains("no longer listed"), "{message}");
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
        approval_head: None,
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
