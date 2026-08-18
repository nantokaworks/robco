use super::*;
use crate::{
    mcp::tools::{ToolError, tests::registry_with_agent},
    overseer::ledger::{Ledger, LedgerPhase},
};

fn ledger_with_entry(agent_id: &str, display_id: &str) -> Ledger {
    Ledger {
        entries: vec![crate::overseer::ledger::LedgerEntry {
            task_id: "task".into(),
            display_id: display_id.into(),
            repo: "/repo".into(),
            agent_id: agent_id.into(),
            branch: "task".into(),
            phase: LedgerPhase::Escalated,
            dispatched_at: Utc::now(),
            settled_at: None,
            retries: 0,
            pr_url: Some("https://pr/1".into()),
            branch_updates: 0,
            merge_recovery: Default::default(),
            merge_hold: Default::default(),
            manual_merge_skip: None,
            merge_hold_cap_escalated: false,
            merge_hold_rechecks: 0,
            merge_hold_recheck_reason: None,
            merge_hold_recheck_head: None,
            prerequisite_wait: None,
            merge_hold_stuck_notified: false,
            escalation_notified_reason: None,
            escalation_notified_head: None,
            worker_escalated: false,
            operator_override: None,
            merge_approval: None,
            pr_facts: None,
        }],
        ..Ledger::default()
    }
}

#[test]
fn a_target_matching_no_ledger_entry_is_refused_without_enqueueing() {
    let error = grant_operator_override(
        "missing",
        || Ok(ledger_with_entry("a1", "#1")),
        |_| panic!("must not enqueue when the target does not resolve"),
    )
    .unwrap_err();

    assert!(error.to_string().contains("no live session"));
}

#[test]
fn a_target_matching_by_agent_id_or_display_id_enqueues_a_bypass_request() {
    for target in ["a1", "#1"] {
        let mut enqueued = None;
        let outcome = grant_operator_override(
            target,
            || Ok(ledger_with_entry("a1", "#1")),
            |request| {
                enqueued = Some(request);
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(outcome["ok"], true);
        assert_eq!(outcome["mode"], "operator_override");
        assert!(matches!(
            enqueued,
            Some(RuntimeRequest::OperatorMergeOverride { .. })
        ));
    }
}

#[test]
fn an_unconfirmed_fallback_is_refused_without_touching_the_ledger_or_queue() {
    let registry = registry_with_agent("a1");
    let error = approve(
        &registry,
        &ApproveArgs {
            agent_id: "a1".into(),
            confirm: false,
        },
    )
    .unwrap_err();

    assert!(matches!(error, ToolError::InvalidParams(_)));
    assert!(error.to_string().contains("confirm must be true"));
}

/// Reproduces the real-world race the fallback exists for: the status
/// snapshot says the agent is still live, but the session is actually gone
/// by the time `send_keys` runs (the worker's tmux session died in the gap
/// between the two calls). `confirm: true` must reach the operator-override
/// path instead of surfacing the raw tmux error.
#[test]
fn a_session_confirmed_gone_after_send_keys_fails_falls_through_to_operator_override() {
    let registry = registry_with_agent("a1");
    let mut enqueued = None;
    let outcome = approve_with(
        &registry,
        &ApproveArgs {
            agent_id: "a1".into(),
            confirm: true,
        },
        |_, _| Status::Running,
        |_| {
            Err(crate::Error::Command {
                context: "tmux send-keys",
                stderr: "can't find session: robco-task".into(),
            })
        },
        |_| Ok(false),
        || Ok(ledger_with_entry("a1", "#1")),
        |request| {
            enqueued = Some(request);
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(outcome["ok"], true);
    assert_eq!(outcome["mode"], "operator_override");
    assert!(matches!(
        enqueued,
        Some(RuntimeRequest::OperatorMergeOverride { .. })
    ));
}

#[test]
fn a_send_keys_failure_with_the_session_still_present_is_not_swallowed() {
    let registry = registry_with_agent("a1");
    let error = approve_with(
        &registry,
        &ApproveArgs {
            agent_id: "a1".into(),
            confirm: true,
        },
        |_, _| Status::Running,
        |_| {
            Err(crate::Error::Command {
                context: "tmux send-keys",
                stderr: "boom".into(),
            })
        },
        |_| Ok(true),
        || panic!("must not load the ledger when the session is genuinely reachable"),
        |_| panic!("must not enqueue when the session is genuinely reachable"),
    )
    .unwrap_err();

    assert!(matches!(error, ToolError::Execution(_)));
    assert!(error.to_string().contains("boom"));
}
