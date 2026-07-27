use super::*;

/// `dropr task ready` is the last CLI surface robco invokes, and the CLI only
/// exposes `task list` and `task ready`. Pinning the argv means the next time
/// that surface moves, this test fails instead of a dispatch pass.
#[test]
fn the_ready_fetch_invokes_a_subcommand_the_cli_exposes() {
    assert_eq!(
        ready_args("ws-1", "20"),
        [
            "task",
            "ready",
            "--workspace",
            "ws-1",
            "--limit",
            "20",
            "--json"
        ]
    );
}

/// The display-row parse, spelled once so each case reads as one call.
fn parse_tasks(raw: &[u8]) -> Option<Vec<DroprTaskCandidate>> {
    parse_as(raw)
}

#[test]
fn parses_ready_tasks_array() {
    let tasks = parse_tasks(
        br##"[{"display_id":"#42","title":"Ship it","priority":"high","status":"ready"}]"##,
    )
    .unwrap();
    assert_eq!(tasks[0].display_id, "#42");
    assert_eq!(tasks[0].title, "Ship it");
    assert_eq!(tasks[0].priority, "high");
    assert_eq!(tasks[0].status, "ready");
}

#[test]
fn parses_ready_tasks_object_and_global_id() {
    let tasks = parse_tasks(
            br##"{"tasks":[{"global_display_id":"#7","title":"Polish UI","priority":"medium","status":"ready"}]}"##,
        )
        .unwrap();
    assert_eq!(tasks[0].display_id, "#7");
}

#[test]
fn skips_malformed_ready_tasks() {
    let tasks = parse_tasks(
        br##"[
                {"display_id":"#1","title":"First"},
                {"display_id":"#2"},
                {"global_display_id":"#3","title":"Third"}
            ]"##,
    )
    .unwrap();
    assert_eq!(
        tasks
            .iter()
            .map(|task| task.display_id.as_str())
            .collect::<Vec<_>>(),
        ["#1", "#3"]
    );
}
#[test]
fn accepts_ready_tasks_without_priority_or_status() {
    let tasks = parse_tasks(br##"[{"display_id":"#42","title":"Ship it"}]"##).unwrap();
    assert_eq!(tasks[0].display_id, "#42");
    assert_eq!(tasks[0].priority, "");
    assert_eq!(tasks[0].status, "");
}
#[test]
fn rejects_malformed_ready_tasks() {
    assert!(parse_tasks(b"not json").is_none());
    assert!(parse_tasks(br#"{"items":[]}"#).is_none());
}
#[test]
fn parses_in_progress_tasks_tolerantly_in_both_shapes() {
    let array =
        parse_tasks(br##"[{"display_id":"#8","title":"Active"},{"display_id":"#9"}]"##).unwrap();
    let object = parse_tasks(
        br##"{"tasks":[{"display_id":"#10","title":"Also active","status":"in_progress"}]}"##,
    )
    .unwrap();
    assert_eq!(array.len(), 1);
    assert_eq!(array[0].priority, "");
    assert_eq!(array[0].status, "");
    assert_eq!(object[0].status, "in_progress");
}
