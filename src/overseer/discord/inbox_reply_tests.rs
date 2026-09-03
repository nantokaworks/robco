use super::*;
use crate::ui::inbox::{InboxItem, InboxKind};
use chrono::Utc;

fn live_escalation(target_id: &str) -> InboxItem {
    InboxItem {
        kind: InboxKind::Escalation,
        repo: None,
        agent_id: None,
        target_session: Some("session".into()),
        target_id: target_id.into(),
        label: "label".into(),
        detail: "worker_blocked".into(),
        at: Utc::now(),
        pr_url: None,
        pr_facts: None,
        sentence: None,
    }
}

#[test]
fn an_empty_inbox_says_so() {
    assert_eq!(format_inbox(&[], Locale::En), "inbox is empty");
}

#[test]
fn a_row_carries_its_move_tag_target_and_guidance() {
    let rendered = format_inbox(&[live_escalation("a1")], Locale::En);
    assert!(rendered.contains("ANSWER"));
    assert!(rendered.contains("a1"));
}

#[test]
fn a_row_names_its_repository() {
    let mut row = live_escalation("a1");
    row.repo = Some("robco".into());
    let rendered = format_inbox(&[row], Locale::En);
    assert!(rendered.contains("ANSWER robco a1"), "{rendered}");
}

#[test]
fn a_row_with_no_matching_ledger_entry_still_renders() {
    let rendered = format_inbox(&[live_escalation("a1")], Locale::En);
    assert!(rendered.contains("ANSWER a1"), "{rendered}");
}

#[test]
fn japanese_locale_translates_the_guidance_but_not_the_tag() {
    let english = format_inbox(&[live_escalation("a1")], Locale::En);
    let japanese = format_inbox(&[live_escalation("a1")], Locale::Ja);
    assert!(japanese.contains("ANSWER"));
    assert_ne!(english, japanese);
}
