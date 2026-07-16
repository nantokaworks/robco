use super::*;

#[test]
fn canonicalizes_common_github_urls() {
    assert_eq!(
        canonical_repo("https://github.com/NantokaWorks/robco.git"),
        Some("github:nantokaworks/robco".to_string())
    );
    assert_eq!(
        canonical_repo("git@github.com:nantokaworks/dropr.git"),
        Some("github:nantokaworks/dropr".to_string())
    );
}

#[test]
fn parses_workspace_line() {
    let line = "  materialised  Xdin9xDHmhuOohKzCBmZX                 dropr                 https://github.com/nantokaworks/dropr.git";
    let workspace = parse_workspace_line(line).unwrap();
    assert_eq!(workspace.kind, "materialised");
    assert_eq!(workspace.name, "dropr");
    assert_eq!(
        workspace.repo_url,
        "https://github.com/nantokaworks/dropr.git"
    );
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
fn task(display_id: &str, status: &str) -> DroprTaskCandidate {
    DroprTaskCandidate {
        display_id: display_id.to_string(),
        title: display_id.to_string(),
        priority: String::new(),
        status: status.to_string(),
    }
}

#[test]
fn merges_repo_task_results() {
    assert_eq!(merge_repo_tasks(None, None), None);
    assert_eq!(
        merge_repo_tasks(None, Some(vec![task("#2", "ready")])).unwrap()[0].display_id,
        "#2"
    );
    assert_eq!(
        merge_repo_tasks(Some(vec![task("#1", "")]), None).unwrap()[0].status,
        "in_progress"
    );

    let tasks = merge_repo_tasks(
        Some(vec![task("#1", "in_progress")]),
        Some(vec![task("#2", "ready")]),
    )
    .unwrap();
    let ids = tasks
        .iter()
        .map(|task| task.display_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, ["#1", "#2"]);
}
