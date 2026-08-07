//! `robco_approve` — confirm a live worker's prompt, or, when no live
//! session is left to confirm into, grant a one-time operator bypass for the
//! judge veto/escalate verdict or autonomy-envelope hard stop currently
//! blocking its pull request instead.
//!
//! The two paths answer the same operator intent — "yes, proceed" — over
//! whichever channel still reaches the work: typing into a live tmux session
//! when one exists, or handing the merge pass a decision to pick up on its
//! next tick when it does not (see
//! `overseer::runtime_request::RuntimeRequest::OperatorMergeOverride`).

use chrono::Utc;
use serde_json::{Value, json};

use crate::{
    model::Status,
    overseer::{
        ledger::Ledger,
        runtime_request::{self, RuntimeRequest},
    },
    registry::Registry,
    tmux,
};

use super::{ToolResult, exec_err, find_agent, live_status};

pub(super) fn approve(registry: &Registry, target: &str) -> ToolResult<Value> {
    if let Ok((repo, agent)) = find_agent(registry, target) {
        let status = live_status(repo, agent).status;
        if !matches!(status, Status::Dead | Status::BranchOnly) {
            tmux::send_keys(&agent.tmux_session, &["y", "Enter"]).map_err(exec_err)?;
            return Ok(json!({ "ok": true, "mode": "session" }));
        }
    }
    grant_operator_override(
        target,
        || Ledger::load().map_err(exec_err),
        |request| runtime_request::enqueue(request).map_err(exec_err),
    )
}

/// The session-less fallback: `target` reaches no live worker, so the
/// decision is handed to the merge pass instead. `load_ledger` and `enqueue`
/// are injected so a test can exercise the validation logic without touching
/// the real ledger file or the runtime-request queue — see `policy::policy`
/// for the same shape.
///
/// Validated against the ledger up front, read-only — the ledger itself is
/// written only inside the daemon's own pass, never from an MCP call — so a
/// typo in `target` fails the call immediately rather than silently
/// enqueueing a request nothing will ever match.
fn grant_operator_override(
    target: &str,
    load_ledger: impl FnOnce() -> ToolResult<Ledger>,
    enqueue: impl FnOnce(RuntimeRequest) -> ToolResult<()>,
) -> ToolResult<Value> {
    let ledger = load_ledger()?;
    if !ledger
        .entries
        .iter()
        .any(|entry| entry.agent_id == target || entry.display_id == target)
    {
        return Err(exec_err(format!(
            "no live session and no ledger entry for {target}"
        )));
    }
    enqueue(RuntimeRequest::OperatorMergeOverride {
        source: "mcp".into(),
        target: target.to_owned(),
        at: Utc::now(),
    })?;
    Ok(json!({
        "ok": true,
        "mode": "operator_override",
        "target": target
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overseer::ledger::{Ledger, LedgerPhase};

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
                merge_judge_fail_safes: 0,
                merge_hold_cap_escalated: false,
                merge_hold_rechecks: 0,
                merge_hold_recheck_reason: None,
                merge_hold_recheck_head: None,
                prerequisite_wait: None,
                merge_hold_stuck_notified: false,
                worker_escalated: false,
                operator_override: None,
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
}
