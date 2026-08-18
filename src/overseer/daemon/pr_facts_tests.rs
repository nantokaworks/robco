use super::*;
use serde_json::json;

#[test]
fn a_full_payload_yields_the_title_size_and_failed_check() {
    let value = json!({
        "title": "Fix the thing",
        "additions": 30,
        "deletions": 12,
        "changedFiles": 4,
        "statusCheckRollup": [
            {"name": "validate / Validate", "conclusion": "FAILURE", "startedAt": "2026-01-01T00:00:00Z"},
            {"name": "build", "conclusion": "SUCCESS", "startedAt": "2026-01-01T00:00:00Z"},
        ],
    });

    let facts = extract(&value).expect("size fields are present");
    assert_eq!(facts.title, "Fix the thing");
    assert_eq!(facts.files_changed, 4);
    assert_eq!(facts.lines_changed, 42);
    assert_eq!(facts.failed_checks, vec!["validate / Validate".to_string()]);
}

#[test]
fn a_payload_missing_the_size_fields_yields_no_facts() {
    assert!(extract(&json!({"title": "Fix the thing"})).is_none());
    assert!(extract(&json!({})).is_none());
}

#[test]
fn a_missing_title_still_yields_facts_with_an_empty_one() {
    let value = json!({"additions": 1, "deletions": 1, "changedFiles": 1});

    let facts = extract(&value).expect("size fields are present");
    assert_eq!(facts.title, "");
    assert!(facts.failed_checks.is_empty());
}

#[test]
fn a_green_rollup_yields_no_failed_checks() {
    let value = json!({
        "additions": 1,
        "deletions": 1,
        "changedFiles": 1,
        "statusCheckRollup": [{"name": "build", "conclusion": "SUCCESS"}],
    });

    assert!(extract(&value).unwrap().failed_checks.is_empty());
}
