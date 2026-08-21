use super::*;
use crate::mcp::tools::tests::registry_with_agent;

#[test]
fn explicit_target_beats_parent_environment() {
    let ids = resolve_identities(Some("explicit"), |key| match key {
        ENV_PARENT_AGENT_ID => Some("parent".to_string()),
        ENV_AGENT_ID => Some("sender".to_string()),
        _ => None,
    })
    .unwrap();
    assert_eq!(ids.target_agent_id, "explicit");
    assert_eq!(ids.sender_agent_id.as_deref(), Some("sender"));
}

#[test]
fn parent_environment_is_used_when_set() {
    let ids = resolve_identities(None, |key| {
        (key == ENV_PARENT_AGENT_ID).then(|| "parent".to_string())
    })
    .unwrap();
    assert_eq!(ids.target_agent_id, "parent");
}

#[test]
fn missing_target_and_parent_defaults_to_overseer() {
    let ids = resolve_identities(None, |_| None).unwrap();
    assert_eq!(ids.target_agent_id, crate::overseer::OVERSEER_AGENT_ID);

    let ids = resolve_identities(None, |_| Some(String::new())).unwrap();
    assert_eq!(ids.target_agent_id, crate::overseer::OVERSEER_AGENT_ID);
}

#[test]
fn self_report_is_rejected() {
    let err = guard_self_report("same", Some("same")).unwrap_err();
    assert!(matches!(err, super::super::ToolError::InvalidParams(_)));
    assert!(guard_self_report("target", Some("sender")).is_ok());
    assert!(guard_self_report("target", None).is_ok());
}

#[test]
fn sender_label_prefers_title_then_raw_id_then_unknown() {
    let registry = registry_with_agent("known");
    assert_eq!(sender_label(&registry, Some("known")), "task");
    assert_eq!(sender_label(&registry, Some("missing")), "missing");
    assert_eq!(sender_label(&registry, None), "unknown");
}

#[test]
fn formats_single_report_line() {
    assert_eq!(
        format_report_line("controller", "finished work"),
        "[robco report from controller] finished work"
    );
}

#[test]
fn multiline_message_is_collapsed_to_one_line() {
    assert_eq!(
        sanitize_message("first line\nsecond\r\n\tthird").unwrap(),
        "first line second third"
    );
}

#[test]
fn control_only_message_is_rejected_as_blank() {
    let err = sanitize_message("\n\r\t\0").unwrap_err();
    assert!(matches!(err, super::super::ToolError::InvalidParams(_)));
    assert_eq!(err.to_string(), "message must not be blank");
}

#[test]
fn normal_message_is_unchanged() {
    assert_eq!(sanitize_message("finished work").unwrap(), "finished work");
}

#[test]
fn guard_rejects_confirmation_and_unavailable_targets() {
    let report = |status, awaiting_confirmation| StatusReport {
        status,
        awaiting_confirmation,
        worktree_missing: false,
        mcp_active: false,
    };
    let busy = report(Status::Waiting, true);
    assert!(
        guard_delivery(busy, true)
            .unwrap_err()
            .to_string()
            .starts_with("target_busy:")
    );

    let dead = report(Status::Dead, false);
    assert!(
        guard_delivery(dead, true)
            .unwrap_err()
            .to_string()
            .starts_with("target_unavailable:")
    );

    let idle = report(Status::Idle, false);
    assert!(
        guard_delivery(idle, false)
            .unwrap_err()
            .to_string()
            .starts_with("target_unavailable:")
    );
    assert!(guard_delivery(idle, true).is_ok());
}

#[test]
fn maps_cli_exit_codes_from_core_error_kinds() {
    assert_eq!(report_exit_code(&invalid_params("invalid")), 3);
    assert_eq!(report_exit_code(&exec_err("target_busy: retry")), 2);
    assert_eq!(report_exit_code(&exec_err("target_unavailable: gone")), 4);
    assert_eq!(report_exit_code(&exec_err("tmux failed")), 4);
}

#[test]
fn overseer_parent_report_appends_to_inbox() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("overseer/inbox.jsonl");
    let lookup = |key: &str| match key {
        ENV_PARENT_AGENT_ID => Some(crate::overseer::OVERSEER_AGENT_ID.into()),
        ENV_AGENT_ID => Some("worker-1".into()),
        _ => None,
    };
    let append = |report: &InboxReport| crate::overseer::inbox::append_report_to(&path, report);
    deliver_report_with_lookup_and_append("turn-done", None, lookup, append).unwrap();

    let line = std::fs::read_to_string(path).unwrap();
    let report: InboxReport = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(report.agent_id, "worker-1");
    assert_eq!(report.kind, crate::overseer::inbox::ReportKind::TurnDone);
}

#[test]
fn legacy_chief_parent_report_appends_to_overseer_inbox() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("overseer/inbox.jsonl");
    let lookup = |key: &str| match key {
        ENV_PARENT_AGENT_ID => Some("chief".into()),
        ENV_AGENT_ID => Some("legacy-worker".into()),
        _ => None,
    };
    let append = |report: &InboxReport| crate::overseer::inbox::append_report_to(&path, report);

    deliver_report_with_lookup_and_append("turn-done", None, lookup, append).unwrap();

    let line = std::fs::read_to_string(path).unwrap();
    let report: InboxReport = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(report.agent_id, "legacy-worker");
}

#[test]
fn missing_target_and_parent_still_delivers_to_overseer_inbox() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("overseer/inbox.jsonl");
    let lookup = |key: &str| match key {
        ENV_AGENT_ID => Some("worker-1".into()),
        _ => None,
    };
    let append = |report: &InboxReport| crate::overseer::inbox::append_report_to(&path, report);

    deliver_report_with_lookup_and_append("done", None, lookup, append).unwrap();

    let line = std::fs::read_to_string(path).unwrap();
    let report: InboxReport = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(report.agent_id, "worker-1");
    assert_eq!(report.kind, crate::overseer::inbox::ReportKind::Done);
}

#[test]
fn missing_sender_still_errors_with_defaulted_target() {
    let lookup = |_: &str| None;
    let append = |_: &InboxReport| Ok(());

    let err = deliver_report_with_lookup_and_append("done", None, lookup, append).unwrap_err();
    assert!(matches!(err, super::super::ToolError::InvalidParams(_)));
    assert!(err.to_string().contains("ROBCO_AGENT_ID"));
}
