use super::*;
use crate::ui::inbox::{InboxItem, InboxKind};
use chrono::Utc;

fn question(target_id: &str) -> InboxItem {
    InboxItem {
        kind: InboxKind::Question,
        target_session: Some("session".into()),
        target_id: target_id.into(),
        label: "label".into(),
        detail: "worker is waiting".into(),
        at: Utc::now(),
    }
}

#[test]
fn an_empty_inbox_says_so() {
    assert_eq!(format_inbox(&[], Locale::En), "inbox is empty");
}

#[test]
fn a_row_carries_its_move_tag_target_and_guidance() {
    let rendered = format_inbox(&[question("a1")], Locale::En);
    assert!(rendered.contains("ANSWER"));
    assert!(rendered.contains("a1"));
}

#[test]
fn japanese_locale_translates_the_guidance_but_not_the_tag() {
    let english = format_inbox(&[question("a1")], Locale::En);
    let japanese = format_inbox(&[question("a1")], Locale::Ja);
    assert!(japanese.contains("ANSWER"));
    assert_ne!(english, japanese);
}
