use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    config::{ENV_AGENT_ID, ENV_PARENT_AGENT_ID},
    model::Status,
    overseer::{inbox::InboxReport, is_overseer_child},
    registry::Registry,
    status::StatusReport,
    tmux,
};

use super::{ToolResult, exec_err, find_agent, invalid_params, live_status, validate_non_blank};

#[derive(Deserialize)]
pub(super) struct ReportArgs {
    message: String,
    target_agent_id: Option<String>,
}

pub(super) fn report(args: ReportArgs) -> ToolResult<Value> {
    deliver_report(&args.message, args.target_agent_id.as_deref())?;
    Ok(json!({ "ok": true }))
}

pub(crate) fn deliver_report(message: &str, target_agent_id: Option<&str>) -> ToolResult<()> {
    deliver_report_with_lookup_and_append(
        message,
        target_agent_id,
        |key| std::env::var(key).ok(),
        crate::overseer::inbox::append_report,
    )
}

fn deliver_report_with_lookup_and_append(
    message: &str,
    target_agent_id: Option<&str>,
    lookup: impl Fn(&str) -> Option<String>,
    append: impl FnOnce(&InboxReport) -> crate::Result<()>,
) -> ToolResult<()> {
    let message = sanitize_message(message)?;
    let identities = resolve_identities(target_agent_id, lookup)?;
    guard_self_report(
        &identities.target_agent_id,
        identities.sender_agent_id.as_deref(),
    )?;

    if is_overseer_child(Some(&identities.target_agent_id)) {
        let agent_id = identities
            .sender_agent_id
            .ok_or_else(|| invalid_params("ROBCO_AGENT_ID is required for overseer reports"))?;
        let report = InboxReport::lifecycle(agent_id, &message)
            .ok_or_else(|| invalid_params("invalid overseer report kind"))?;
        return append(&report).map_err(exec_err);
    }

    let registry = Registry::load().map_err(exec_err)?;
    let (repo, target) = find_agent(&registry, &identities.target_agent_id)?;
    let status = live_status(repo, target);
    let session_exists = tmux::has_session(&target.tmux_session).map_err(exec_err)?;
    guard_delivery(status, session_exists)?;

    let sender = sender_label(&registry, identities.sender_agent_id.as_deref());
    let line = format_report_line(&sender, &message);
    tmux::send_literal_text(&target.tmux_session, &line).map_err(exec_err)?;
    tmux::send_keys(&target.tmux_session, &["Enter"]).map_err(exec_err)?;
    Ok(())
}

pub(crate) fn report_exit_code(error: &super::ToolError) -> u8 {
    match error {
        super::ToolError::InvalidParams(_) => 3,
        super::ToolError::Execution(message) if message.starts_with("target_busy:") => 2,
        super::ToolError::Execution(_) => 4,
    }
}

fn sanitize_message(message: &str) -> ToolResult<String> {
    let mut sanitized = String::with_capacity(message.len());
    let mut previous_was_control = false;

    for character in message.chars() {
        if character.is_control() {
            if !previous_was_control {
                sanitized.push(' ');
            }
            previous_was_control = true;
        } else {
            sanitized.push(character);
            previous_was_control = false;
        }
    }

    let sanitized = sanitized.trim().to_string();
    validate_non_blank("message", &sanitized)?;
    Ok(sanitized)
}

#[derive(Debug)]
struct ReportIdentities {
    target_agent_id: String,
    sender_agent_id: Option<String>,
}

fn guard_self_report(target_agent_id: &str, sender_agent_id: Option<&str>) -> ToolResult<()> {
    if sender_agent_id == Some(target_agent_id) {
        return Err(invalid_params("cannot report to the calling agent"));
    }
    Ok(())
}

fn resolve_identities(
    explicit_target: Option<&str>,
    lookup: impl Fn(&str) -> Option<String>,
) -> ToolResult<ReportIdentities> {
    let target_agent_id = match explicit_target {
        Some(target) => {
            validate_non_blank("target_agent_id", target)?;
            target.to_string()
        }
        None => non_empty(lookup(ENV_PARENT_AGENT_ID)).ok_or_else(|| {
            invalid_params("no report target is configured; provide target_agent_id or set ROBCO_PARENT_AGENT_ID")
        })?,
    };
    Ok(ReportIdentities {
        target_agent_id,
        sender_agent_id: non_empty(lookup(ENV_AGENT_ID)),
    })
}

fn sender_label(registry: &Registry, sender_agent_id: Option<&str>) -> String {
    sender_agent_id.map_or_else(
        || "unknown".to_string(),
        |sender_id| {
            registry
                .repos
                .iter()
                .flat_map(|repo| &repo.agents)
                .find(|agent| agent.id == sender_id)
                .map_or_else(|| sender_id.to_string(), |agent| agent.title.clone())
        },
    )
}

fn format_report_line(sender: &str, message: &str) -> String {
    format!("[robco report from {sender}] {message}")
}

fn guard_delivery(report: StatusReport, session_exists: bool) -> ToolResult<()> {
    if report.awaiting_confirmation {
        return Err(exec_err(
            "target_busy: target is awaiting a confirmation response",
        ));
    }
    if report.status == Status::Dead || !session_exists {
        return Err(exec_err("target_unavailable: target session is not live"));
    }
    Ok(())
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
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
    fn parent_environment_is_used_and_empty_is_unset() {
        let ids = resolve_identities(None, |key| {
            (key == ENV_PARENT_AGENT_ID).then(|| "parent".to_string())
        })
        .unwrap();
        assert_eq!(ids.target_agent_id, "parent");

        let err = resolve_identities(None, |_| Some(String::new())).unwrap_err();
        assert!(matches!(err, super::super::ToolError::InvalidParams(_)));
        assert!(err.to_string().contains("no report target is configured"));
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
}
