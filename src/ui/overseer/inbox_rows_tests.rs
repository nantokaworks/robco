use super::*;
use crate::{
    config::Config,
    model::OverseerCategory,
    registry::Registry,
    ui::inbox::{InboxItem, InboxKind},
};

/// A reason far wider than any sidebar row, so the assertion that the pane
/// carries it in full is not satisfied by a lucky trim.
const LONG_REASON: &str = "merge blocked: the base branch has no required status checks \
                           and the diff touches CI configuration, which the conservative \
                           autonomy level always escalates";

fn item(kind: InboxKind, target_id: &str, session: Option<&str>) -> InboxItem {
    InboxItem {
        kind,
        target_session: session.map(ToString::to_string),
        target_id: target_id.into(),
        label: format!("{target_id} — {LONG_REASON}"),
        detail: LONG_REASON.into(),
        at: chrono::Utc::now(),
    }
}

fn inbox_app(items: Vec<InboxItem>) -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = true;
    app.overseer_inbox = items;
    app.set_overseer_category_expanded(OverseerCategory::Inbox, true);
    app
}

fn rendered(lines: &[Line<'static>]) -> Vec<String> {
    lines.iter().map(ToString::to_string).collect()
}

#[test]
fn the_category_expands_straight_into_item_rows() {
    let app = inbox_app(vec![
        item(InboxKind::Escalation, "#159", None),
        item(InboxKind::Question, "agent-1", Some("robco-agent-1")),
    ]);
    let lines = rendered(&detail_lines(&app));

    // No count header ahead of the items: item `n` is row `n`, which is what
    // the OVERSEER frame's scroll arithmetic relies on.
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("[ESC] #159"), "{lines:?}");
    assert!(lines[1].contains("[?] agent-1"), "{lines:?}");
    assert!(!lines.iter().any(|line| line.contains("inbox (")));
}

#[test]
fn an_empty_inbox_renders_its_empty_state() {
    assert_eq!(rendered(&detail_lines(&inbox_app(Vec::new()))), ["none"]);
}

#[test]
fn the_preview_describes_the_selected_item_and_not_the_other_items() {
    let app = inbox_app(vec![
        item(InboxKind::Escalation, "#159", Some("robco-agent-1")),
        item(InboxKind::Question, "agent-2", Some("robco-agent-2")),
    ]);

    let (title, text) = item_preview(&app, 0);
    let body = rendered(&text.lines).join("\n");

    assert_eq!(title, "OVERSEER / Inbox / #159");
    assert!(body.contains("kind: escalation"), "{body}");
    assert!(body.contains("target: #159"), "{body}");
    assert!(body.contains("session: robco-agent-1"), "{body}");
    // The whole reason, not the row label the sidebar trims to its width.
    assert!(body.contains(LONG_REASON), "{body}");
    // The other item is already on screen in the left frame; repeating it here
    // is what this pane used to do and says nothing about the selected row.
    assert!(!body.contains("agent-2"), "{body}");
}

#[test]
fn a_display_only_item_says_why_it_cannot_be_answered() {
    let app = inbox_app(vec![item(InboxKind::Escalation, "#159", None)]);

    let (_, text) = item_preview(&app, 0);
    let body = rendered(&text.lines).join("\n");

    assert!(
        body.contains("display-only — no live session to answer or approve"),
        "{body}"
    );
}

#[test]
fn a_stale_index_previews_a_notice_rather_than_panicking() {
    // The list re-aggregates under the cursor, so an index can outlive its row
    // between the key press that moved the cursor and the next draw.
    let app = inbox_app(Vec::new());

    let (title, text) = item_preview(&app, 3);

    assert_eq!(title, "OVERSEER / Inbox");
    assert_eq!(rendered(&text.lines), ["item is no longer listed"]);
}
