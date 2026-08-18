use chrono::TimeZone;
use crossterm::event::KeyCode;

use super::*;
use crate::{
    config::Config,
    model::{OverseerCategory, Selection},
    registry::Registry,
    ui::inbox::{InboxItem, InboxKind},
    ui::input::overseer::handle_normal,
};

fn at(second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, second).unwrap()
}

fn item(kind: InboxKind, target_id: &str, second: u32) -> InboxItem {
    InboxItem {
        kind,
        target_session: None,
        target_id: target_id.into(),
        label: format!("{target_id} — escalated"),
        detail: "needs user".into(),
        at: at(second),
        pr_url: None,
        pr_facts: None,
        sentence: None,
    }
}

/// An app with two listed rows and the cursor on the first of them.
///
/// Nothing here writes: the tests below cover which rows a key press names and
/// how the confirmation is routed. Persistence and the suppression window are
/// covered against explicit paths in `crate::overseer::dismissals`, because the
/// real store lives under the operator's `~/.robco` home.
fn inbox_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = true;
    app.overseer_inbox = vec![
        item(InboxKind::Escalation, "#159", 20),
        item(InboxKind::Escalation, "agent-1", 10),
    ];
    app.set_overseer_category_expanded(OverseerCategory::Inbox, true);
    app.selected = app
        .visible()
        .iter()
        .position(|row| matches!(row, Selection::OverseerInbox(0)))
        .expect("no inbox item row");
    app
}

#[test]
fn dismissing_one_row_names_that_row_alone_with_the_timestamp_it_carries() {
    let app = inbox_app();

    assert_eq!(
        app.inbox_dismissal_rows(Some(1)),
        vec![("ESC", "agent-1".to_string(), at(10))]
    );
    // Out of range: the list re-aggregates under the cursor, so an index can
    // outlive the row it pointed at.
    assert!(app.inbox_dismissal_rows(Some(9)).is_empty());
}

#[test]
fn clearing_names_every_listed_row_by_its_own_identity() {
    let app = inbox_app();

    assert_eq!(
        app.inbox_dismissal_rows(None),
        vec![
            ("ESC", "#159".to_string(), at(20)),
            ("ESC", "agent-1".to_string(), at(10)),
        ]
    );
}

#[test]
fn the_clear_key_confirms_before_it_acts() {
    let mut app = inbox_app();

    assert!(handle_normal(&mut app, KeyCode::Char('D')));
    assert!(matches!(
        app.mode,
        Mode::ConfirmInboxDismissAll { count: 2 }
    ));
}

#[test]
fn the_clear_key_reaches_the_confirm_from_the_inbox_category_row_too() {
    let mut app = inbox_app();
    app.selected = app
        .visible()
        .iter()
        .position(|row| *row == Selection::OverseerCategory(OverseerCategory::Inbox))
        .expect("no Inbox category row");

    assert!(handle_normal(&mut app, KeyCode::Char('D')));
    assert!(matches!(
        app.mode,
        Mode::ConfirmInboxDismissAll { count: 2 }
    ));
}

#[test]
fn clearing_an_empty_inbox_says_so_instead_of_opening_a_dialog() {
    let mut app = inbox_app();
    app.overseer_inbox.clear();

    app.confirm_dismiss_inbox();

    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("inbox is already empty")
    );
}

#[test]
fn the_clear_key_is_not_offered_from_another_overseer_category() {
    let mut app = inbox_app();
    app.selected = app
        .visible()
        .iter()
        .position(|row| *row == Selection::OverseerCategory(OverseerCategory::Health))
        .expect("no Health category row");

    assert!(!handle_normal(&mut app, KeyCode::Char('D')));
    assert!(matches!(app.mode, Mode::Normal));
}
