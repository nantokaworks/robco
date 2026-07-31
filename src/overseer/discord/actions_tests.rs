use super::*;

#[test]
fn audit_entries_identify_discord_user() {
    let entry = audit_entry(&Command::Skip("task-1".into()), "user-7", "failed: denied");
    assert_eq!(entry.source.as_deref(), Some("discord"));
    assert_eq!(entry.user_id.as_deref(), Some("user-7"));
    assert_eq!(entry.task.as_deref(), Some("task-1"));
    assert!(entry.reason.contains("failed: denied"));
}

#[test]
fn status_line_reports_no_switch_the_daemon_ignores() {
    let config = OverseerConfig::default();
    assert!(config.dispatch_enabled);
    let line = status_line(&config, 1, 4);
    assert_eq!(
        line,
        "**dispatch** on\n**automerge** off\n**autonomy** conservative\n**workers** 1/3\n**today** 4/20"
    );
    // A dispatching daemon must never be described as switched off.
    assert!(!line.contains("overseer=off"));
}

#[test]
fn audit_reasons_carry_no_debug_output() {
    let entry = audit_entry(
        &Command::TaskCreate {
            repo: "acme/widgets".into(),
            title: "T".into(),
            description: None,
        },
        "user-7",
        "succeeded",
    );
    assert_eq!(
        entry.reason,
        "command succeeded: create task \"T\" in acme/widgets"
    );
}

#[test]
fn overflowing_rows_collapse_into_a_more_tail() {
    let rows: Vec<_> = (0..100)
        .map(|index| format!("row-{index:03} {}", "x".repeat(40)))
        .collect();
    let bounded = bounded_rows(&rows);
    assert!(bounded.chars().count() <= RESPONSE_BUDGET + 20);
    assert!(bounded.lines().last().unwrap().starts_with("… and "));
    assert!(bounded.lines().last().unwrap().ends_with(" more"));
}

#[test]
fn short_rows_render_without_a_tail() {
    let rows = vec!["a".to_string(), "b".to_string()];
    assert_eq!(bounded_rows(&rows), "a\nb");
    assert_eq!(code_block(&rows), "```text\na\nb\n```");
}
