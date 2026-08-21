use super::*;

/// A payload missing a required field must name which action it was, not
/// just the missing field — otherwise an operator has to read the source to
/// tell `dropr_scribble_create` and `dropr_task_status_update` apart.
#[test]
fn missing_content_field_names_the_action_in_the_warning_not_the_rejection() {
    // A missing field is a schema mismatch, not a policy rejection — see
    // `ParseError`'s doc comment. `parse` recovers in place: `outcome` and
    // `reason` still come through, and the unusable action surfaces as
    // `action_error` instead of failing the whole result. See dropr:401.
    let raw = br#"{
        "outcome":"resolved",
        "action":{"name":"dropr_scribble_create","task_id":"task-1"},
        "reason":"note the block"
    }"#;
    let parsed =
        parse(raw, Some("task-1"), "worker-1", &|_| false).expect("recovers, not rejected");
    assert_eq!(parsed.action, None);
    let warning = parsed.action_error.expect("names the schema mismatch");
    assert!(warning.contains("dropr_scribble_create") && warning.contains("content"));
}

#[test]
fn missing_task_id_field_names_the_action_in_the_warning_not_the_rejection() {
    let raw = br#"{
        "outcome":"resolved",
        "action":{"name":"dropr_task_status_update","status":"open"},
        "reason":"release lock"
    }"#;
    let parsed =
        parse(raw, Some("task-1"), "worker-1", &|_| false).expect("recovers, not rejected");
    assert_eq!(parsed.action, None);
    let warning = parsed.action_error.expect("names the schema mismatch");
    assert!(warning.contains("dropr_task_status_update") && warning.contains("task_id"));
}

#[test]
fn live_worker_prevents_task_lock_release() {
    let raw = br#"{
        "outcome":"resolved",
        "action":{"name":"dropr_task_status_update","task_id":"task-1","status":"ready"},
        "reason":"release"
    }"#;
    let rejected = parse(raw, Some("task-1"), "worker-1", &|_| true);
    assert!(
        matches!(rejected, Err(ParseError::RejectedAction(message)) if message.contains("alive"))
    );
    assert!(parse(raw, Some("task-1"), "worker-1", &|_| false).is_ok());
}

/// A case with no known dropr task (`own_task: None` — see
/// `ExceptionCase::dropr_task_id`) has no task lock to release, so a model
/// naming any `task_id` for `dropr_task_status_update` must be rejected
/// outright, not compared against a fabricated identity (dropr:535).
#[test]
fn no_dropr_task_rejects_task_status_update() {
    let raw = br#"{
        "outcome":"resolved",
        "action":{"name":"dropr_task_status_update","task_id":"task-1","status":"ready"},
        "reason":"release"
    }"#;
    let rejected = parse(raw, None, "worker-1", &|_| false);
    assert!(
        matches!(rejected, Err(ParseError::RejectedAction(message)) if message.contains("task lock"))
    );
}
