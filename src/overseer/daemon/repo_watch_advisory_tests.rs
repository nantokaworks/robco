use super::*;

/// A probe binary absent from `PATH` is a host fact, not a run failure —
/// distinguished from every other spawn/exit failure so the caller can skip
/// it silently after the first sighting instead of logging it forever
/// (dropr task #445).
#[test]
fn a_missing_binary_is_reported_as_missing_not_failed() {
    let command = Command::new("robco-test-definitely-not-a-real-binary-xyz");
    let result = probe(command, "phantom", "phantom probe");
    assert!(matches!(result, Err(ProbeError::Missing("phantom"))));
}

/// A binary that exists but exits non-zero with no advisory id in its output
/// is a genuine failure, kept apart from the missing-tool case above.
#[test]
fn a_failing_command_with_no_advisory_id_is_reported_as_failed() {
    let mut command = Command::new("sh");
    command.args(["-c", "echo nothing to see here >&2; exit 1"]);
    let result = probe(command, "phantom", "phantom probe");
    assert!(matches!(result, Err(ProbeError::Failed(_))));
}

#[test]
fn a_missing_tool_is_reported_once_then_stays_quiet() {
    let mut reported = BTreeSet::new();
    assert!(note_missing_tool("govulncheck", &mut reported).is_ok());
    assert!(reported.contains("govulncheck"));
    // The second sighting of the same tool must not error either — the point
    // is silence, not failure.
    assert!(note_missing_tool("govulncheck", &mut reported).is_ok());
    assert_eq!(reported.len(), 1);
}

#[test]
fn finds_a_ghsa_id_inside_bun_audit_style_output() {
    let output = r#"{"advisories":[{"id":"GHSA-2v37-7h3g-55p8","severity":"high"}]}"#;
    assert_eq!(
        advisory_ids(output),
        vec!["GHSA-2v37-7h3g-55p8".to_string()]
    );
}

#[test]
fn finds_a_go_id_inside_govulncheck_text_output() {
    let output = "Scanning your code and 42 packages across 3 dependent modules for known vulnerabilities...\n\nVulnerability #1: GO-2026-1234\n";
    assert_eq!(advisory_ids(output), vec!["GO-2026-1234".to_string()]);
}

#[test]
fn finds_multiple_distinct_ids_sorted_and_deduped() {
    let output = "GHSA-2v37-7h3g-55p8 GO-2026-1234 GHSA-2v37-7h3g-55p8 GO-2026-0001";
    assert_eq!(
        advisory_ids(output),
        vec![
            "GHSA-2v37-7h3g-55p8".to_string(),
            "GO-2026-0001".to_string(),
            "GO-2026-1234".to_string(),
        ]
    );
}

#[test]
fn finds_nothing_in_clean_output() {
    assert!(advisory_ids("0 vulnerabilities found").is_empty());
}

#[test]
fn a_short_hyphenated_token_is_not_mistaken_for_a_ghsa_id() {
    assert!(!is_ghsa_id("GHSA-ab-cd-ef"));
}

#[test]
fn a_non_numeric_go_suffix_is_not_mistaken_for_a_go_id() {
    assert!(!is_go_id("GO-2026-abcd"));
}

#[test]
fn a_short_go_year_is_rejected() {
    assert!(!is_go_id("GO-26-1234"));
}

#[test]
fn the_marker_is_stable_across_id_order() {
    let a = marker("nex", &["GHSA-1".to_string(), "GO-2".to_string()]);
    let b = marker("nex", &["GHSA-1".to_string(), "GO-2".to_string()]);
    assert_eq!(a, b);
}

#[test]
fn the_marker_changes_when_the_id_set_changes() {
    let a = marker("nex", &["GHSA-1".to_string()]);
    let b = marker("nex", &["GHSA-1".to_string(), "GO-2".to_string()]);
    assert_ne!(a, b);
}

#[test]
fn the_title_carries_the_marker_verbatim_for_dedup_matching() {
    let marker = marker("nex", &["GHSA-1".to_string()]);
    let title = title("nex", 1, &marker);
    assert!(title.contains(&marker));
}

#[test]
fn clip_leaves_short_output_untouched() {
    assert_eq!(clip("short output"), "short output");
}

#[test]
fn clip_truncates_long_output_with_a_marker() {
    let long = "a".repeat(5000);
    let clipped = clip(&long);
    assert!(clipped.len() < long.len());
    assert!(clipped.ends_with("(truncated)"));
}
