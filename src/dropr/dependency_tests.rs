use super::*;

#[test]
fn the_dependency_fetch_invokes_a_subcommand_the_cli_exposes() {
    assert_eq!(
        dependency_list_args("task-1"),
        ["task", "dependency", "list", "--task", "task-1", "--json"]
    );
}

#[test]
fn finds_the_blocking_edge_among_several() {
    let raw = br##"[
        {"task_id":"task-a","task_display_id":"#9","depends_on_display_id":"#1","blocking":false,"kind":"related"},
        {"task_id":"task-a","task_display_id":"#9","depends_on_display_id":"#2","blocking":true,"kind":"blocks"}
    ]"##;
    let blocking = parse_blocking(raw, "task-a").unwrap().unwrap();
    assert_eq!(blocking.display_id, "#2");
}

#[test]
fn an_edge_where_the_probed_task_is_the_prerequisite_side_is_not_blocking() {
    // Regression for dropr:382. `dropr task dependency list` returns edges
    // touching the task on either side. When task B waits for task A, probing
    // A finds that same edge; reading it as "A is blocked" held A's own pull
    // request on a wait that belongs to B. Only an edge whose dependent
    // (`task_id`) side is the probed task may hold it.
    let raw = br##"[
        {"task_id":"task-b","task_display_id":"#9","depends_on_task_id":"task-a","depends_on_display_id":"#7","blocking":true,"kind":"blocks"}
    ]"##;
    assert!(parse_blocking(raw, "task-a").unwrap().is_none());
}

#[test]
fn the_probed_task_also_matches_by_display_id() {
    let raw = br##"[
        {"task_id":"task-a","task_display_id":"#9","depends_on_display_id":"#7","blocking":true,"kind":"blocks"}
    ]"##;
    let blocking = parse_blocking(raw, "#9").unwrap().unwrap();
    assert_eq!(blocking.display_id, "#7");
}

#[test]
fn no_blocking_edge_is_none_rather_than_a_parse_failure() {
    let raw = br##"[{"task_id":"task-a","task_display_id":"#9","depends_on_display_id":"#1","blocking":false,"kind":"related"}]"##;
    assert!(parse_blocking(raw, "task-a").unwrap().is_none());
}

#[test]
fn an_empty_edge_list_is_not_blocking() {
    assert!(parse_blocking(b"[]", "task-a").unwrap().is_none());
}

#[test]
fn malformed_json_is_a_parse_failure_not_an_absent_prerequisite() {
    // A probe that could not be read must not be read as "nothing is
    // blocking" — see `daemon::merge_dependency::gate`, which holds the pull
    // request on a probe failure rather than letting it merge past an
    // ordering constraint it never actually confirmed was clear.
    assert!(parse_blocking(b"not json", "task-a").is_none());
}
