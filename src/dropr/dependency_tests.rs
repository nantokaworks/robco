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
        {"depends_on_display_id":"#1","blocking":false,"kind":"related"},
        {"depends_on_display_id":"#2","blocking":true,"kind":"blocks"}
    ]"##;
    let blocking = parse_blocking(raw).unwrap().unwrap();
    assert_eq!(blocking.display_id, "#2");
}

#[test]
fn no_blocking_edge_is_none_rather_than_a_parse_failure() {
    let raw = br##"[{"depends_on_display_id":"#1","blocking":false,"kind":"related"}]"##;
    assert!(parse_blocking(raw).unwrap().is_none());
}

#[test]
fn an_empty_edge_list_is_not_blocking() {
    assert!(parse_blocking(b"[]").unwrap().is_none());
}

#[test]
fn malformed_json_is_a_parse_failure_not_an_absent_prerequisite() {
    // A probe that could not be read must not be read as "nothing is
    // blocking" — see `daemon::merge_dependency::gate`, which holds the pull
    // request on a probe failure rather than letting it merge past an
    // ordering constraint it never actually confirmed was clear.
    assert!(parse_blocking(b"not json").is_none());
}
