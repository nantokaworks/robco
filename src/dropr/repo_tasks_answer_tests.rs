//! Reading one `task_list` answer: what a single reply turns into, and how
//! an unusable one is reported rather than swallowed.

use super::*;

#[test]
fn a_refusal_reaches_the_operator_with_its_message() {
    let refused = read_rows(ToolOutcome::Refused(
        "{\"code\":\"workspace_not_found\",\n  \"reason\":\"unknown\"}".to_owned(),
    ));

    assert_eq!(
        refused.unwrap_err(),
        r#"dropr refused: {"code":"workspace_not_found", "reason":"unknown"}"#
    );
}

#[test]
fn an_unreadable_payload_is_reported_rather_than_read_as_empty() {
    let unreadable = read_rows(ToolOutcome::Ok(json!({ "items": [] })));

    assert!(unreadable.unwrap_err().contains("cannot read"));
}

#[test]
fn rows_that_do_not_deserialize_do_not_discard_their_neighbours() {
    let payload = json!({ "tasks": [
        { "global_display_id": "#1", "title": "Good", "child_count": 2, "id": "id-1" },
        { "global_display_id": "#2" },
        { "display_id": "#3", "title": "Also good" },
    ]});

    let rows = rows(&payload).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].child_count, 2);
    assert_eq!(rows[1].task.display_id, "#3");
}

#[test]
fn a_display_id_sorts_by_its_number_and_tolerates_a_step_suffix() {
    assert_eq!(display_number("#9"), 9);
    assert_eq!(display_number("#42-1"), 42);
    assert_eq!(display_number("not-a-number"), u64::MAX);
}

#[test]
fn a_multiline_message_is_squashed_onto_one_row() {
    assert_eq!(one_line("boom\n  again\tand again"), "boom again and again");
    assert_eq!(one_line(&"x".repeat(200)).chars().count(), 120);
}
