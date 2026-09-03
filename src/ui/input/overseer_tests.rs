use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::super::inbox_respond::DISPLAY_ONLY;
use super::*;
use crate::{config::Config, model::OverseerCategory, registry::Registry, ui::PreviewPane};

#[test]
fn remote_control_i_opens_the_session_prompt() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = false;
    app.orphans.clear();
    app.hosts = vec![crate::ui::actions::remote_hosts::HostSlot::connected(
        crate::model::HostLabel {
            name: "Prod".into(),
            ssh: "prod".into(),
        },
    )];
    app.sync_remote_host_views();
    app.selected = 0;

    assert!(handle_normal(&mut app, KeyCode::Char('i')));
    let expected = crate::overseer::control_session_name(&app.config.tmux_session_prefix);
    assert!(matches!(&app.mode, Mode::PromptSession { session, .. } if session == &expected));
}

#[test]
fn enter_submits_trimmed_instruction() {
    let mut input = TextInput::from("  review task  ");
    let action = prompt_action(
        &mut input,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    );
    assert!(matches!(action, PromptAction::Submit(text) if text == "review task"));
}

#[test]
fn editing_keys_reach_the_shared_buffer_instead_of_appending() {
    let mut input = TextInput::from("review task");

    for code in [KeyCode::Home, KeyCode::Delete, KeyCode::Char('p')] {
        let action = prompt_action(&mut input, KeyEvent::new(code, KeyModifiers::NONE));
        assert!(matches!(action, PromptAction::Stay));
    }

    assert_eq!(input, *"peview task");
}

fn inbox_app(target_session: Option<&str>) -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = true;
    app.overseer_inbox = vec![crate::ui::inbox::InboxItem {
        kind: crate::ui::inbox::InboxKind::Escalation,
        repo: None,
        target_session: target_session.map(ToString::to_string),
        target_id: "agent-1".into(),
        label: "agent-1 — worker".into(),
        detail: "worker_blocked".into(),
        at: chrono::Utc::now(),
        pr_url: None,
        pr_facts: None,
        sentence: None,
    }];
    app.set_overseer_category_expanded(OverseerCategory::Inbox, true);
    app.selected = app
        .visible()
        .iter()
        .position(|row| matches!(row, Selection::OverseerInbox(0)))
        .expect("no inbox item row");
    app
}

#[test]
fn the_removed_second_cursor_keys_bind_to_nothing() {
    // `[` / `]` drove the retired `overseer_inbox_selected` index. The tree's
    // own j/k cursor replaces them, so nothing may claim these keys.
    let mut app = inbox_app(Some("robco-agent-1"));

    assert!(!handle_normal(&mut app, KeyCode::Char('[')));
    assert!(!handle_normal(&mut app, KeyCode::Char(']')));
    assert!(matches!(app.mode, Mode::Normal));
}

#[test]
fn the_old_answer_key_reports_instead_of_opening_the_add_repo_prompt() {
    // `a` is the global clone / add-repository key and used to answer the
    // inbox. From an inbox row it must reach neither outcome.
    let mut app = inbox_app(Some("robco-agent-1"));

    press(&mut app, KeyCode::Char('a'));

    assert!(
        matches!(app.mode, Mode::Normal),
        "a from an inbox row opened a dialog"
    );
    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("press enter to answer the selected inbox item")
    );
}

#[test]
fn approve_acts_on_the_selected_row_from_any_preview_tab() {
    for pane in [PreviewPane::Info, PreviewPane::Claude] {
        let mut app = inbox_app(None);
        app.preview = pane;

        // A display-only row has no session to approve into, and says so
        // rather than silently doing nothing — which is what the key reports
        // from every tab, so the tab is never what decides the outcome.
        assert!(handle_normal(&mut app, KeyCode::Char('y')));
        assert_eq!(
            app.message.as_ref().map(|(message, _)| message.as_str()),
            Some(DISPLAY_ONLY),
            "preview tab {pane:?}"
        );
    }
}

fn press(app: &mut App, code: KeyCode) {
    assert!(
        !app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
            .unwrap(),
        "key {code:?} quit the app"
    );
}

#[test]
fn enter_opens_the_answer_prompt_for_an_actionable_row() {
    let mut app = inbox_app(Some("robco-agent-1"));

    press(&mut app, KeyCode::Enter);

    match &app.mode {
        Mode::PromptInbox { item, input } => {
            assert_eq!(item.target_session.as_deref(), Some("robco-agent-1"));
            assert_eq!(item.label, "agent-1 — worker");
            assert!(input.text().is_empty());
        }
        _ => panic!("enter did not open the answer prompt"),
    }
}

#[test]
fn enter_on_a_display_only_row_explains_itself_and_never_attaches() {
    let mut app = inbox_app(None);

    press(&mut app, KeyCode::Enter);

    assert!(
        matches!(app.mode, Mode::Normal),
        "a display-only row must not open a prompt or attach"
    );
    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some(DISPLAY_ONLY)
    );
}

#[test]
fn stop_key_opens_panic_confirm_from_any_overseer_tab() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = true;
    app.selected = 0; // First OVERSEER row (the control AI).
    // Works regardless of the active preview tab.
    app.preview = PreviewPane::Claude;

    assert!(handle_normal(&mut app, KeyCode::Char('S')));
    assert!(matches!(app.mode, Mode::ConfirmOverseerPanic));
}

#[test]
fn stop_key_is_ignored_when_overseer_inactive() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = false;

    assert!(!handle_normal(&mut app, KeyCode::Char('S')));
    assert!(matches!(app.mode, Mode::Normal));
}

#[test]
fn reset_key_reports_when_the_daemon_is_already_alive() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = true;
    app.overseer_snapshot.daemon_alive = true;

    assert!(handle_normal(&mut app, KeyCode::Char('R')));
    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("overseer daemon is already running")
    );
}

#[test]
fn reset_key_starts_the_daemon_when_it_is_dead() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = true;
    assert!(!app.overseer_snapshot.daemon_alive);

    assert!(handle_normal(&mut app, KeyCode::Char('R')));

    assert!(matches!(app.mode, Mode::Normal));
    // No launchd service is installed in the test environment, so this
    // exercises the "no service" branch of `start_daemon`.
    let message = app
        .message
        .as_ref()
        .map(|(message, _)| message.as_str())
        .unwrap_or_default();
    assert_ne!(message, "overseer daemon is already running");
}

#[test]
fn reset_key_is_ignored_when_overseer_inactive() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = false;

    assert!(!handle_normal(&mut app, KeyCode::Char('R')));
    assert!(matches!(app.mode, Mode::Normal));
}

#[test]
fn daemon_stop_key_opens_confirm_when_the_daemon_is_alive() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = true;
    app.overseer_snapshot.daemon_alive = true;

    assert!(handle_normal(&mut app, KeyCode::Char('K')));
    assert!(matches!(app.mode, Mode::ConfirmDaemonStop));
}

#[test]
fn daemon_stop_key_reports_when_the_daemon_is_already_dead() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = true;
    assert!(!app.overseer_snapshot.daemon_alive);

    assert!(handle_normal(&mut app, KeyCode::Char('K')));

    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("overseer daemon is not running")
    );
}

#[test]
fn daemon_stop_key_is_ignored_when_overseer_inactive() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = false;
    app.overseer_snapshot.daemon_alive = true;

    assert!(!handle_normal(&mut app, KeyCode::Char('K')));
    assert!(matches!(app.mode, Mode::Normal));
}

#[test]
fn stop_key_opens_panic_confirm_off_the_overseer_rows() {
    // Regression for the worker-row case (#175): S is an overseer-wide stop
    // and no longer requires the selection to be an OVERSEER category. Any row
    // while the panel is active reaches the confirm — the S branch keys off
    // `overseer_visible` alone and never inspects the selection, so a worker
    // row (Selection::Agent) takes this same path.
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = true;
    // Point the cursor past the OVERSEER category rows so the selection is
    // not one of them.
    app.selected = 999;
    assert!(
        !matches!(app.selected_item(), Some(Selection::OverseerCategory(_))),
        "precondition: selection must not be an overseer category row"
    );

    assert!(handle_normal(&mut app, KeyCode::Char('S')));
    assert!(matches!(app.mode, Mode::ConfirmOverseerPanic));
}
