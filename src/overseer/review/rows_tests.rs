use std::collections::BTreeMap;

use super::*;
use crate::{
    overseer::{ledger::PrFacts, row_summaries::RowSummaries},
    ui::inbox::{InboxItem, InboxKind},
};

fn item(target_id: &str, detail: &str, facts: Option<PrFacts>) -> InboxItem {
    InboxItem {
        kind: InboxKind::Escalation,
        repo: None,
        target_session: None,
        target_id: target_id.into(),
        label: target_id.into(),
        detail: detail.into(),
        at: chrono::Utc::now(),
        pr_url: None,
        pr_facts: facts,
        sentence: None,
    }
}

#[test]
fn a_row_with_facts_carries_its_title_size_and_failed_checks() {
    let items = vec![item(
        "#159",
        "autonomy_envelope",
        Some(PrFacts {
            title: "Add merge approval queue".into(),
            files_changed: 53,
            lines_changed: 612,
            failed_checks: vec!["validate / Validate".into()],
        }),
    )];

    let cases = cases(&items);

    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0].id, "#159");
    assert_eq!(cases[0].reason, "autonomy_envelope");
    assert_eq!(
        cases[0].pr_title.as_deref(),
        Some("Add merge approval queue")
    );
    assert_eq!(cases[0].files_changed, Some(53));
    assert_eq!(cases[0].lines_changed, Some(612));
    assert_eq!(
        cases[0].failed_checks,
        vec!["validate / Validate".to_string()]
    );
}

#[test]
fn a_row_with_no_facts_carries_only_its_reason() {
    let items = vec![item("#1", "checks_waiting", None)];

    let cases = cases(&items);

    assert_eq!(cases[0].pr_title, None);
    assert_eq!(cases[0].files_changed, None);
    assert!(cases[0].failed_checks.is_empty());
}

#[test]
fn a_blank_title_reads_as_no_title() {
    let items = vec![item(
        "#1",
        "checks_not_green",
        Some(PrFacts {
            title: String::new(),
            files_changed: 1,
            lines_changed: 1,
            failed_checks: Vec::new(),
        }),
    )];

    assert_eq!(cases(&items)[0].pr_title, None);
}

#[test]
fn rows_past_the_cap_are_left_out() {
    let items: Vec<_> = (0..MAX_ROWS + 5)
        .map(|n| item(&format!("#{n}"), "checks_waiting", None))
        .collect();

    assert_eq!(cases(&items).len(), MAX_ROWS);
}

#[test]
fn an_overlong_reason_is_truncated_with_an_ellipsis() {
    let items = vec![item("#1", &"x".repeat(MAX_REASON_CHARS * 2), None)];

    let reason = &cases(&items)[0].reason;
    assert!(reason.ends_with('…'));
    assert_eq!(reason.chars().count(), MAX_REASON_CHARS + 1);
}

fn answer(id: &str, sentence: &str) -> RowAnswer {
    RowAnswer {
        id: id.into(),
        sentence: sentence.into(),
    }
}

#[test]
fn an_answer_is_stored_under_the_signature_it_was_pinned_with() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("row_summaries.json");
    let pending = BTreeMap::from([("#159".to_string(), "sig-a".to_string())]);

    apply(
        &path,
        pending,
        &[answer("#159", "adds a merge approval queue")],
    )
    .unwrap();

    let stored = RowSummaries::load_from(&path).unwrap();
    assert_eq!(
        stored.get("#159", "sig-a"),
        Some("adds a merge approval queue")
    );
}

/// Nothing stops a model from echoing back an id it invented — `pending` is
/// the only authority on which rows this pass actually offered it.
#[test]
fn an_answer_for_an_id_not_in_pending_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("row_summaries.json");
    let pending = BTreeMap::from([("#159".to_string(), "sig-a".to_string())]);

    apply(&path, pending, &[answer("#999", "not a real row")]).unwrap();

    let stored = RowSummaries::load_from(&path).unwrap();
    assert!(stored.get("#999", "sig-a").is_none());
}

#[test]
fn a_target_no_longer_pending_loses_its_stored_summary() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("row_summaries.json");
    apply(
        &path,
        BTreeMap::from([("#1".to_string(), "sig-1".to_string())]),
        &[answer("#1", "first pass")],
    )
    .unwrap();

    // A later pass no longer sees "#1" in the Inbox at all.
    apply(
        &path,
        BTreeMap::from([("#2".to_string(), "sig-2".to_string())]),
        &[answer("#2", "second pass")],
    )
    .unwrap();

    let stored = RowSummaries::load_from(&path).unwrap();
    assert!(stored.get("#1", "sig-1").is_none());
    assert_eq!(stored.get("#2", "sig-2"), Some("second pass"));
}
