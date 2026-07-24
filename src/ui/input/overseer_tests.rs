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

#[test]
fn inbox_navigation_is_handled_from_category_selection() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = true;
    app.selected = OverseerCategory::Inbox.index();
    app.preview = PreviewPane::Info;

    assert!(handle_normal(&mut app, KeyCode::Char('[')));
    assert!(handle_normal(&mut app, KeyCode::Char(']')));
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
