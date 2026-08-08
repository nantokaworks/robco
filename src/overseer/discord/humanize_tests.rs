use super::*;

#[test]
fn a_daemon_event_name_maps_to_a_sentence() {
    assert_eq!(
        sentence("pr_opened").as_deref(),
        Some("The worker opened a pull request.")
    );
}

#[test]
fn a_hold_code_maps_to_a_sentence() {
    assert_eq!(
        sentence("checks_not_green").as_deref(),
        Some("A required check failed on the pull request.")
    );
}

#[test]
fn a_prefixed_code_maps_regardless_of_its_detail() {
    assert_eq!(
        sentence("merge_hold_cap_reached:checks_waiting").as_deref(),
        Some("The pull request was held on the same condition too many times.")
    );
    assert_eq!(
        sentence("claimed_elsewhere:agent-1").as_deref(),
        Some("Another agent already holds this task.")
    );
    assert_eq!(
        sentence("prerequisite_unmerged:#42").as_deref(),
        Some("This task waits for another task's pull request to merge first.")
    );
}

#[test]
fn an_unknown_code_is_not_mapped() {
    assert_eq!(sentence("some_future_code:detail"), None);
}

#[test]
fn free_form_prose_is_not_mapped() {
    assert_eq!(sentence("worker blocked"), None);
    assert_eq!(sentence("failure threshold reached"), None);
}

#[test]
fn a_judge_wrapper_unwraps_to_the_judge_own_words() {
    assert_eq!(
        sentence("judge_escalate:The change needs a rebase first.").as_deref(),
        Some("The change needs a rebase first.")
    );
}

#[test]
fn an_empty_judge_wrapper_is_not_mapped() {
    assert_eq!(sentence("judge_escalate:"), None);
    assert_eq!(sentence("judge_veto:   "), None);
}

#[test]
fn a_release_pipeline_wrapper_unwraps_to_its_own_words() {
    assert_eq!(
        sentence("release_published:Published RobCo 0.1.93.").as_deref(),
        Some("Published RobCo 0.1.93.")
    );
    assert_eq!(
        sentence("release_failed:Failed at stage=check cargo test failed.").as_deref(),
        Some("Failed at stage=check cargo test failed.")
    );
}

#[test]
fn surrounding_whitespace_does_not_defeat_the_lookup() {
    assert!(sentence("  merged  ").is_some());
}

#[test]
fn a_release_pipeline_skip_reason_maps_to_a_sentence() {
    assert_eq!(
        sentence("release_pipeline_skipped:checkout_not_on_merged_commit").as_deref(),
        Some("The release checkout is not on the merged commit, so the release did not run.")
    );
    assert_eq!(
        sentence("release_pipeline_skipped:working_tree_dirty").as_deref(),
        Some("The release checkout has uncommitted changes, so the release did not run.")
    );
}

/// dropr:429 — a checkout off `main` gets its own sentence, distinct from
/// the generic "not on the merged commit" one.
#[test]
fn a_release_pipeline_skip_reason_names_a_checkout_off_main() {
    assert_eq!(
        sentence("release_pipeline_skipped:checkout_not_on_main").as_deref(),
        Some("The release checkout is on another branch, not main, so the release did not run.")
    );
    assert_eq!(
        sentence("release_pipeline_skipped:checkout_detached").as_deref(),
        Some("The release checkout has a detached HEAD, so the release did not run.")
    );
}
