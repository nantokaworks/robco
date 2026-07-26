use std::cell::RefCell;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::{config::Config, model::OverseerCategory, registry::Registry};

#[test]
fn enter_submits_trimmed_instruction() {
    let mut input = "  review task  ".to_string();
    let action = prompt_action(
        &mut input,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    );
    assert!(matches!(action, PromptAction::Submit(text) if text == "review task"));
}

#[test]
fn answer_and_approve_use_existing_tmux_sequences() {
    let calls = RefCell::new(Vec::new());
    send_response(
        "target",
        InboxResponse::Answer("ship it"),
        |session, text| {
            calls.borrow_mut().push(format!("literal:{session}:{text}"));
            Ok(())
        },
        |session, keys| {
            calls
                .borrow_mut()
                .push(format!("keys:{session}:{}", keys.join(",")));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        calls.borrow().as_slice(),
        ["literal:target:ship it", "keys:target:Enter"]
    );

    calls.borrow_mut().clear();
    send_response(
        "target",
        InboxResponse::Approve,
        |_, _| Ok(()),
        |session, keys| {
            calls
                .borrow_mut()
                .push(format!("keys:{session}:{}", keys.join(",")));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(calls.borrow().as_slice(), ["keys:target:y,Enter"]);
}

fn inbox_app(target_session: Option<&str>) -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = true;
    app.overseer_inbox = vec![crate::ui::inbox::InboxItem {
        kind: crate::ui::inbox::InboxKind::Question,
        target_session: target_session.map(ToString::to_string),
        target_id: "agent-1".into(),
        label: "agent-1 — worker".into(),
        detail: "worker is waiting on a confirmation prompt: worker".into(),
        at: chrono::Utc::now(),
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
        Mode::PromptInbox {
            target_session,
            label,
            input,
        } => {
            assert_eq!(target_session, "robco-agent-1");
            assert_eq!(label, "agent-1 — worker");
            assert!(input.is_empty());
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
    app.selected = 0; // First OVERSEER category row.
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
fn reset_key_opens_confirm_when_circuit_is_open() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = true;
    app.overseer_snapshot.overseer.failure_circuit_threshold = 2;
    app.overseer_snapshot.ledger.counters.consecutive_failures = 2;

    assert!(handle_normal(&mut app, KeyCode::Char('R')));
    assert!(matches!(app.mode, Mode::ConfirmOverseerReset));
}

#[test]
fn reset_key_reports_when_circuit_is_closed() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = true;
    app.overseer_snapshot.overseer.failure_circuit_threshold = 2;
    app.overseer_snapshot.ledger.counters.consecutive_failures = 1;

    assert!(handle_normal(&mut app, KeyCode::Char('R')));
    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("circuit is closed; nothing to reset")
    );
}

#[test]
fn reset_key_is_ignored_when_overseer_inactive() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = false;
    app.overseer_snapshot.overseer.failure_circuit_threshold = 2;
    app.overseer_snapshot.ledger.counters.consecutive_failures = 2;

    assert!(!handle_normal(&mut app, KeyCode::Char('R')));
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
