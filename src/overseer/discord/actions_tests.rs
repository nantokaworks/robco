use super::*;

#[test]
fn audit_entries_identify_discord_user() {
    let entry = audit_entry(
        &Command::Skip("task-1".into()),
        "user-7",
        "discord",
        "failed: denied",
    );
    assert_eq!(entry.source.as_deref(), Some("discord"));
    assert_eq!(entry.user_id.as_deref(), Some("user-7"));
    assert_eq!(entry.task.as_deref(), Some("task-1"));
    assert!(entry.reason.contains("failed: denied"));
}

#[test]
fn audit_entries_identify_the_mcp_caller() {
    let entry = audit_entry(&Command::Help, "mcp", "mcp", "succeeded");
    assert_eq!(entry.source.as_deref(), Some("mcp"));
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
        "discord",
        "succeeded",
    );
    assert_eq!(
        entry.reason,
        "command succeeded: create task \"T\" in acme/widgets"
    );
}
