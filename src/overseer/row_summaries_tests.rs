use super::*;

fn summary(sentence: &str, signature: &str) -> RowSummary {
    RowSummary {
        sentence: sentence.into(),
        signature: signature.into(),
        generated_at: Utc::now(),
    }
}

#[test]
fn a_missing_file_loads_as_empty() {
    let dir = tempfile::tempdir().unwrap();
    let table = RowSummaries::load_from(&dir.path().join("row_summaries.json")).unwrap();
    assert!(table.get("#1", "sig").is_none());
}

#[test]
fn a_stored_sentence_round_trips_through_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("row_summaries.json");
    let mut table = RowSummaries::default();
    table.upsert(
        "#159".into(),
        summary("adds a merge approval queue", "sig-a"),
    );
    table.save_to(&path).unwrap();

    let reloaded = RowSummaries::load_from(&path).unwrap();
    assert_eq!(
        reloaded.get("#159", "sig-a"),
        Some("adds a merge approval queue")
    );
}

#[test]
fn a_mismatched_signature_returns_nothing() {
    let mut table = RowSummaries::default();
    table.upsert(
        "#159".into(),
        summary("adds a merge approval queue", "sig-a"),
    );

    // The row has since moved on to a new revision — the stored sentence
    // must not be read back as though it still described the current case.
    assert!(table.get("#159", "sig-b").is_none());
}

#[test]
fn retain_live_drops_targets_the_caller_no_longer_vouches_for() {
    let mut table = RowSummaries::default();
    table.upsert("#1".into(), summary("one", "sig"));
    table.upsert("#2".into(), summary("two", "sig"));

    table.retain_live(|target_id| target_id == "#1");

    assert!(table.get("#1", "sig").is_some());
    assert!(table.get("#2", "sig").is_none());
}

#[test]
fn a_corrupt_file_loads_as_empty_rather_than_failing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("row_summaries.json");
    fs::write(&path, b"not json").unwrap();

    let table = RowSummaries::load_from(&path).unwrap();
    assert!(table.get("#1", "sig").is_none());
}
