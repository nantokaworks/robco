use std::cell::RefCell;

use super::*;
use crate::{
    config::Config,
    model::{OverseerCategory, Selection},
    registry::Registry,
    ui::inbox::{InboxItem, InboxKind},
};

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

/// An app whose Inbox lists one actionable row for `target_id`.
fn inbox_app(target_id: &str) -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = true;
    app.overseer_inbox = vec![InboxItem {
        kind: InboxKind::Escalation,
        target_session: Some(format!("robco-{target_id}")),
        target_id: target_id.into(),
        label: format!("{target_id} — worker"),
        detail: "worker_blocked".into(),
        at: chrono::Utc::now(),
        pr_url: None,
        pr_facts: None,
    }];
    app.set_overseer_category_expanded(OverseerCategory::Inbox, true);
    app.selected = app
        .visible()
        .iter()
        .position(|row| matches!(row, Selection::OverseerInbox(0)))
        .expect("no inbox item row");
    app
}

fn tmux_failure() -> crate::Error {
    crate::Error::Command {
        context: "tmux",
        stderr: "boom".into(),
    }
}

/// The suppression the acknowledgement recorded, straight off the store the
/// aggregation reads. The displayed list cannot carry this assertion by
/// itself: the test app's snapshot re-derives from empty sources, so the row
/// leaves the list whether or not anything was recorded.
fn suppressed(item: &InboxItem) -> bool {
    crate::overseer::dismissals::Dismissals::load()
        .unwrap()
        .suppresses(item.kind.code(), &item.target_id, item.at)
}

/// The suppression store's lock file needs its directory, which a real
/// installation has from the overseer's first run but the isolated test home
/// starts without.
///
/// Also serializes the tests that write to the store: every process-wide write
/// prunes entries whose target is not in the writing app's (here: empty) live
/// set, so a concurrent writer would delete the entry this test is about to
/// assert on.
fn lock_overseer_home() -> std::sync::MutexGuard<'static, ()> {
    static STORE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    std::fs::create_dir_all(crate::overseer::overseer_home().unwrap()).unwrap();
    guard
}

#[test]
fn a_successful_approve_marks_the_row_handled() {
    let _store = lock_overseer_home();
    let mut app = inbox_app("agent-approve-ok");
    let item = app.overseer_inbox[0].clone();

    app.approve_inbox_with(
        0,
        |_, _| Ok(()),
        |_, keys| {
            assert_eq!(keys, ["y", "Enter"]);
            Ok(())
        },
    );

    assert!(
        suppressed(&item),
        "approve did not record the acknowledgement suppression"
    );
    assert!(
        !app.overseer_inbox.iter().any(|row| row == &item),
        "the approved row is still listed"
    );
    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("approval sent")
    );
    // The suppression is bounded by the row's own timestamp: the same target
    // escalating again later must come back.
    let newer = InboxItem {
        at: item.at + chrono::Duration::seconds(1),
        ..item
    };
    assert!(!suppressed(&newer), "a fresh escalation would stay hidden");
}

#[test]
fn a_failed_approve_leaves_the_row_untouched() {
    let mut app = inbox_app("agent-approve-err");
    let item = app.overseer_inbox[0].clone();

    app.approve_inbox_with(0, |_, _| Ok(()), |_, _| Err(tmux_failure()));

    assert!(
        !suppressed(&item),
        "a failed send must not suppress the row"
    );
    assert_eq!(app.overseer_inbox, vec![item]);
}

#[test]
fn a_successful_answer_marks_the_row_handled() {
    let _store = lock_overseer_home();
    let mut app = inbox_app("agent-answer-ok");
    let item = app.overseer_inbox[0].clone();

    app.answer_inbox_with(
        &item,
        "ship it",
        |_, text| {
            assert_eq!(text, "ship it");
            Ok(())
        },
        |_, _| Ok(()),
    );

    assert!(
        suppressed(&item),
        "answer did not record the acknowledgement suppression"
    );
    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("answer sent")
    );
}

#[test]
fn a_failed_answer_leaves_the_row_untouched() {
    let mut app = inbox_app("agent-answer-err");
    let item = app.overseer_inbox[0].clone();

    app.answer_inbox_with(&item, "ship it", |_, _| Err(tmux_failure()), |_, _| Ok(()));

    assert!(
        !suppressed(&item),
        "a failed send must not suppress the row"
    );
    assert_eq!(app.overseer_inbox, vec![item]);
}
