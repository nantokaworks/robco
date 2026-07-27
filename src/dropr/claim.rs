//! Reading and taking a dropr task claim.
//!
//! `dropr task ready` lists only unclaimed work, but the list goes stale the
//! moment it is fetched. These calls let a caller re-read one task's claim and
//! then take it atomically, so "this task is free" and "this task is mine"
//! stop being two decisions minutes apart.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    mcp::{ToolOutcome, call_tool},
    writes::accepted,
};

/// The claim fields of a single dropr task, as `task_list` reports them.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct TaskClaim {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub claimed_by_agent_id: Option<String>,
    #[serde(default)]
    pub claim_expires_at: Option<DateTime<Utc>>,
}

impl TaskClaim {
    /// True while `agent` is not the holder and the recorded claim has not
    /// lapsed. A claim with no expiry counts as live: an unknown deadline is
    /// not an expired one.
    pub fn held_by_other(&self, agent: &str, now: DateTime<Utc>) -> bool {
        self.holder(agent).is_some() && self.claim_expires_at.is_none_or(|expires| expires > now)
    }

    /// The holding agent when it is somebody other than `agent`.
    pub fn holder(&self, agent: &str) -> Option<&str> {
        self.claimed_by_agent_id
            .as_deref()
            .filter(|holder| !holder.is_empty() && *holder != agent)
    }
}

/// Outcome of a targeted claim attempt.
pub enum ClaimAttempt {
    Claimed,
    /// dropr answered and declined; carries its machine-readable reason.
    Refused(String),
    /// The call never reached a verdict — the CLI is missing, hung, or broken.
    Unavailable,
}

#[derive(Deserialize)]
struct TaskListPayload {
    #[serde(default)]
    tasks: Vec<TaskClaim>,
}

/// Re-reads one task's claim. `None` when dropr could not be reached.
pub fn task_claim(workspace_id: &str, task_id: &str, timeout: Duration) -> Option<TaskClaim> {
    let payload = match call_tool(
        "task_list",
        json!({ "workspace_id": workspace_id, "task_id": task_id, "limit": 1 }),
        timeout,
    )? {
        ToolOutcome::Ok(payload) => payload,
        ToolOutcome::Refused(_) => return None,
    };
    serde_json::from_value::<TaskListPayload>(payload)
        .ok()?
        .tasks
        .into_iter()
        .next()
}

/// Claims exactly `task_id` for `agent_id`. Targeted `task_next` never falls
/// back to another task, so a refusal means this task is not ours to take.
pub fn claim_task(
    workspace_id: &str,
    task_id: &str,
    agent_id: &str,
    timeout: Duration,
) -> ClaimAttempt {
    match call_tool(
        "task_next",
        json!({
            "workspace_id": workspace_id,
            "agent_id": agent_id,
            "task_id": task_id,
            "limit": 1,
        }),
        timeout,
    ) {
        // An answer that carries no candidate claimed nothing, whatever else it
        // says; treating it as a claim would put a worker back on a task no lock
        // is holding for it.
        Some(ToolOutcome::Ok(payload)) if claimed(&payload) => ClaimAttempt::Claimed,
        Some(ToolOutcome::Ok(_)) => ClaimAttempt::Refused("no_candidate".into()),
        Some(ToolOutcome::Refused(message)) => ClaimAttempt::Refused(refusal_reason(&message)),
        None => ClaimAttempt::Unavailable,
    }
}

/// Whether a `task_next` answer actually handed us a task.
fn claimed(payload: &Value) -> bool {
    payload
        .get("candidates")
        .and_then(Value::as_array)
        .is_some_and(|candidates| !candidates.is_empty())
}

/// Hands a claim back, for the case where the worker it was taken for never
/// started. Best effort: a failure here costs the task its claim TTL, not
/// correctness.
pub fn release_claim(workspace_id: &str, task_id: &str, agent_id: &str, timeout: Duration) -> bool {
    let outcome = call_tool(
        "task_status_update",
        json!({
            "items": [{
                "workspace_id": workspace_id,
                "task_id": task_id,
                "agent_id": agent_id,
                "status": "open",
                "note": "overseer released the claim: worker spawn failed",
            }],
        }),
        timeout,
    );
    match outcome {
        // The bulk call answers per item, so the envelope alone proves nothing:
        // a transition the wrong agent asked for fails inside a call that
        // succeeded. See [`accepted`].
        Some(ToolOutcome::Ok(payload)) => accepted(&payload),
        _ => false,
    }
}

/// Compacts a refusal into something a decision log line can carry: dropr
/// wraps a JSON body in the JSON-RPC error message.
fn refusal_reason(message: &str) -> String {
    let compact = serde_json::from_str::<Value>(message)
        .ok()
        .and_then(|body| {
            body.get("reason")
                .or_else(|| body.get("code"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| message.to_owned());
    compact
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(80)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(minutes: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(minutes * 60, 0).unwrap()
    }

    fn claim(holder: &str, expires: Option<DateTime<Utc>>) -> TaskClaim {
        TaskClaim {
            status: "in_progress".into(),
            claimed_by_agent_id: Some(holder.into()),
            claim_expires_at: expires,
        }
    }

    #[test]
    fn a_live_claim_by_another_agent_is_held() {
        let claim = claim("manual-run", Some(at(30)));
        assert!(claim.held_by_other("overseer", at(10)));
        assert_eq!(claim.holder("overseer"), Some("manual-run"));
    }

    #[test]
    fn a_lapsed_claim_is_no_longer_held() {
        let claim = claim("manual-run", Some(at(30)));
        assert!(!claim.held_by_other("overseer", at(31)));
        // The holder still names who let it lapse, so the takeover is loggable.
        assert_eq!(claim.holder("overseer"), Some("manual-run"));
    }

    #[test]
    fn a_claim_without_an_expiry_counts_as_live() {
        assert!(claim("manual-run", None).held_by_other("overseer", at(9_999)));
    }

    #[test]
    fn our_own_claim_is_not_somebody_elses() {
        let claim = claim("overseer", Some(at(30)));
        assert!(!claim.held_by_other("overseer", at(10)));
        assert_eq!(claim.holder("overseer"), None);
    }

    #[test]
    fn refusals_compact_to_the_servers_reason() {
        assert_eq!(
            refusal_reason(r#"{"code":"task_not_claimable","reason":"claimed"}"#),
            "claimed"
        );
        assert_eq!(
            refusal_reason(r#"{"code":"task_not_claimable"}"#),
            "task_not_claimable"
        );
        // Non-JSON messages still have to survive as one log-safe line.
        assert_eq!(refusal_reason("boom\n  again"), "boom again");
    }

    #[test]
    fn an_answer_without_a_candidate_claimed_nothing() {
        assert!(claimed(
            &json!({ "candidates": [{ "task": {"id": "task-1"} }] })
        ));
        assert!(!claimed(&json!({ "candidates": [] })));
        assert!(!claimed(&json!({ "recommended_index": 0 })));
    }

    #[test]
    fn a_per_item_rejection_is_not_a_release() {
        // The bulk call succeeds while the item inside it fails; reading only the
        // envelope would report a claim handed back that is still held.
        assert!(!accepted(&json!({
            "results": [{
                "id": "task-1",
                "status": "error",
                "error": {"message": "only the locking agent can release this lock"},
            }],
        })));
        assert!(accepted(&json!({
            "results": [{ "id": "task-1", "status": "ok" }],
        })));
    }

    #[test]
    fn a_task_list_payload_yields_the_claim() {
        let payload = json!({
            "tasks": [{
                "status": "in_progress",
                "claimed_by_agent_id": "manual-run",
                "claim_expires_at": "2026-07-24T11:42:02.329Z",
                "title": "unrelated field",
            }],
        });
        let claim = serde_json::from_value::<TaskListPayload>(payload)
            .unwrap()
            .tasks
            .swap_remove(0);
        assert_eq!(claim.claimed_by_agent_id.as_deref(), Some("manual-run"));
        assert!(claim.claim_expires_at.is_some());
    }
}
