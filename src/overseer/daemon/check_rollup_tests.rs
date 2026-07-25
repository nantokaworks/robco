use super::*;
use serde_json::json;

/// One check run of the workflow the branch ruleset actually requires.
fn run_of(conclusion: Value, started_at: &str) -> Value {
    json!({
        "__typename": "CheckRun",
        "name": "validate / Validate",
        "workflowName": "Basic Checks",
        "conclusion": conclusion,
        "startedAt": started_at,
    })
}

#[test]
fn a_terminal_conclusion_that_is_not_a_failure_does_not_hold() {
    // A required workflow a path filter excluded never reports again. Reading its
    // conclusion as still-running holds the pull request for ever.
    for conclusion in ["SKIPPED", "NEUTRAL"] {
        assert_eq!(
            classify(&[
                json!({"name": "validate / Validate", "conclusion": conclusion}),
                json!({"name": "docs / Lint", "conclusion": "SUCCESS"}),
            ]),
            Checks::Green,
            "expected {conclusion} not to hold the merge"
        );
    }
}

#[test]
fn every_terminal_non_success_conclusion_is_a_failure() {
    for conclusion in FAILED_CONCLUSIONS {
        assert_eq!(
            classify(&[json!({"name": "validate / Validate", "conclusion": conclusion})]),
            Checks::Failed,
            "expected {conclusion} to read as failed"
        );
    }
}

#[test]
fn a_check_still_running_has_not_failed() {
    // Merge recovery spends a worker turn on a red head. A head whose checks have
    // not finished yet must not read as red, or every pull request would be handed
    // back once before its very first check reported.
    for in_flight in [
        json!({"name": "validate / Validate", "conclusion": null, "status": "QUEUED"}),
        json!({"name": "validate / Validate", "status": "IN_PROGRESS"}),
        json!({"context": "ci/deploy", "state": "PENDING"}),
    ] {
        assert_eq!(
            classify(&[
                json!({"name": "docs / Lint", "conclusion": "SUCCESS"}),
                in_flight
            ]),
            Checks::Waiting
        );
    }
    // A rollup with no entries describes a head whose checks have not been created.
    assert_eq!(classify(&[]), Checks::Waiting);
}

#[test]
fn a_superseded_run_does_not_veto_the_run_that_replaced_it() {
    // Both are runs of the one check the ruleset requires, and the ruleset reads the
    // newer of them. A duplicate the concurrency group cancelled, or one left queued
    // behind the run that overtook it, is a fact about an abandoned run rather than
    // about the head.
    for superseded in [
        run_of(json!("CANCELLED"), "2026-07-25T08:52:41Z"),
        json!({"name": "validate / Validate", "conclusion": null, "status": "QUEUED"}),
    ] {
        assert_eq!(
            classify(&[superseded, run_of(json!("SUCCESS"), "2026-07-25T08:52:43Z")]),
            Checks::Green
        );
    }
}

#[test]
fn the_most_recent_run_of_a_check_decides_it() {
    // A re-run that failed is the answer branch protection reads, so an earlier
    // success of the same check must not merge a red head.
    assert_eq!(
        classify(&[
            run_of(json!("SUCCESS"), "2026-07-25T08:52:41Z"),
            run_of(json!("FAILURE"), "2026-07-25T09:20:04Z"),
        ]),
        Checks::Failed
    );
}

#[test]
fn runs_of_one_check_that_started_together_hold_until_both_report() {
    // Duplicate triggers of one workflow start within the same second, so their
    // start times cannot order them. The gate holds rather than merging on the half
    // that happens to have finished.
    assert_eq!(
        classify(&[
            run_of(json!("SUCCESS"), "2026-07-25T08:52:41Z"),
            run_of(json!(null), "2026-07-25T08:52:41Z"),
        ]),
        Checks::Waiting
    );
    // Once both have reported, the cancelled half is the one that carries no answer.
    assert_eq!(
        classify(&[
            run_of(json!("SUCCESS"), "2026-07-25T08:52:41Z"),
            run_of(json!("CANCELLED"), "2026-07-25T08:52:41Z"),
        ]),
        Checks::Green
    );
}

#[test]
fn an_entry_without_a_name_stands_alone() {
    // Nameless entries cannot be matched to each other, so neither may swallow the
    // other's answer.
    assert_eq!(
        classify(&[
            json!({"conclusion": "SUCCESS"}),
            json!({"conclusion": "FAILURE"})
        ]),
        Checks::Failed
    );
    assert_eq!(
        classify(&[
            json!({"conclusion": "SUCCESS"}),
            json!({"conclusion": null})
        ]),
        Checks::Waiting
    );
}

#[test]
fn a_commit_status_reports_under_its_context() {
    // A commit status names its check `context` and dates it `createdAt`, and the
    // most recent post of one context is the one that counts.
    assert_eq!(
        classify(&[
            json!({"context": "ci/lint", "state": "FAILURE", "createdAt": "2026-07-25T08:00:00Z"}),
            json!({"context": "ci/lint", "state": "SUCCESS", "createdAt": "2026-07-25T09:00:00Z"}),
        ]),
        Checks::Green
    );
}
