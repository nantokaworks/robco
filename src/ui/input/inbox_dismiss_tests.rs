use super::*;
use crate::{
    config::Config,
    registry::Registry,
    ui::inbox::{InboxItem, InboxKind},
};
use chrono::TimeZone;

fn at(second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, second).unwrap()
}

fn item(kind: InboxKind, target_id: &str, second: u32) -> InboxItem {
    InboxItem {
        kind,
        repo: None,
        agent_id: None,
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
    app
}

#[test]
fn dismissing_one_row_names_that_row_alone_with_the_timestamp_it_carries() {
    let app = inbox_app();

    assert_eq!(
        app.inbox_dismissal_rows(1),
        vec![("ESC", "agent-1".to_string(), at(10))]
    );
    // Out of range: the list re-aggregates under the cursor, so an index can
    // outlive the row it pointed at.
    assert!(app.inbox_dismissal_rows(9).is_empty());
}
