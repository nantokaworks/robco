//! What Overseer writes back onto a dropr task.
//!
//! Both writes go over `dropr mcp-stdio` rather than the `dropr` CLI. The CLI
//! exposes `task list` and `task ready` and nothing else, so the
//! `dropr scribble create` and `dropr task status update` argv these calls used
//! to build named subcommands that do not exist: every note Overseer believed
//! it had left was an `unrecognized subcommand` error folded into a log line.
//! The MCP server carries the whole task API, and robco already speaks it for
//! claims and for the task pane.
//!
//! Both tools take a bulk envelope and answer per item, so a rejected write
//! arrives as `status: "error"` inside an otherwise successful call. Reading
//! only the envelope is how a lost note reads as a recorded one.

use std::{fmt, time::Duration};

use serde_json::{Value, json};

use super::mcp::{ToolOutcome, call_tool};
use crate::overseer::OVERSEER_AGENT_ID;

/// Longest server message a decision-log reason carries. Past this the line
/// stops being readable in an alert digest and starts being a wall.
const MESSAGE_LIMIT: usize = 160;

/// Why a write-back did not land.
///
/// The two cases need different responses, so they stay apart all the way into
/// the decision log: a refusal repeats until somebody changes robco or the
/// workspace, while an unavailable dropr may well answer the next pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    /// dropr answered and declined — an unknown tool, an argument shape it does
    /// not accept, a transition it will not make.
    Refused(String),
    /// The call reached no verdict: the binary is missing, the session died, or
    /// it outlived its timeout.
    Unavailable,
}

impl fmt::Display for WriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(message) => write!(formatter, "refused: {message}"),
            Self::Unavailable => formatter.write_str(
                "unavailable: dropr mcp-stdio gave no answer — the binary is missing, \
                 it died, or it timed out",
            ),
        }
    }
}

pub type WriteResult = std::result::Result<(), WriteError>;

/// Leaves one note on a task's blackboard.
pub(crate) fn scribble_create_timeout(task_id: &str, body: &str, timeout: Duration) -> WriteResult {
    write(
        "scribble_create",
        scribble_arguments(task_id, body),
        timeout,
    )
}

/// Moves one task's lifecycle status.
pub(crate) fn task_status_update_timeout(
    task_id: &str,
    status: &str,
    timeout: Duration,
) -> WriteResult {
    write(
        "task_status_update",
        status_arguments(task_id, status),
        timeout,
    )
}

/// Files a new task onto a workspace's board.
///
/// A blank `workspace_id` is refused here rather than round-tripped to the
/// server: dropr's `task_create` answers a blank workspace target the same
/// way it answers no target at all — "exactly one of workspace_id,
/// canonical_repo, repo_url must be set" — which reads like a caller bug in
/// dropr rather than the empty value it actually is (dropr task #445).
pub(crate) fn task_create_timeout(
    workspace_id: &str,
    title: &str,
    description: Option<&str>,
    timeout: Duration,
) -> WriteResult {
    if workspace_id.trim().is_empty() {
        return Err(WriteError::Refused(
            "task_create_timeout: workspace_id is blank".to_string(),
        ));
    }
    write(
        "task_create",
        task_create_arguments(workspace_id, title, description),
        timeout,
    )
}

fn scribble_arguments(task_id: &str, body: &str) -> Value {
    json!({
        "items": [{
            "task_id": task_id,
            "agent_id": OVERSEER_AGENT_ID,
            "kind": "note",
            "body": body,
        }],
    })
}

fn status_arguments(task_id: &str, status: &str) -> Value {
    json!({
        "items": [{
            "task_id": task_id,
            // Overseer holds the claim being released, and dropr only lets the
            // holder move the task, so the write has to name it.
            "agent_id": OVERSEER_AGENT_ID,
            "status": lifecycle_status(status),
        }],
    })
}

fn task_create_arguments(workspace_id: &str, title: &str, description: Option<&str>) -> Value {
    let mut item = json!({
        "workspace_id": workspace_id,
        "agent_id": OVERSEER_AGENT_ID,
        "title": title,
    });
    if let Some(description) = description {
        item["description"] = Value::String(description.to_string());
    }
    json!({ "items": [item] })
}

/// Triage spells the released state `ready`; dropr's lifecycle has no such
/// status and answers `invalid transition: in_progress -> ready`. `open` is the
/// state that releases the lock, so the translation happens here rather than by
/// narrowing the vocabulary triage is allowed to ask for.
fn lifecycle_status(status: &str) -> &str {
    match status {
        "ready" => "open",
        other => other,
    }
}

fn write(tool: &'static str, arguments: Value, timeout: Duration) -> WriteResult {
    verdict(call_tool(tool, arguments, timeout))
}

/// Reads one write's answer.
///
/// Three ways to fail and only one of them looks like failure from the outside:
/// dropr can decline the call, decline the item inside an accepted call, or
/// never answer at all.
fn verdict(outcome: Option<ToolOutcome>) -> WriteResult {
    match outcome {
        Some(ToolOutcome::Ok(payload)) if accepted(&payload) => Ok(()),
        Some(ToolOutcome::Ok(payload)) => Err(WriteError::Refused(item_refusal(&payload))),
        Some(ToolOutcome::Refused(message)) => Err(WriteError::Refused(one_line(&message))),
        None => Err(WriteError::Unavailable),
    }
}

/// Whether every item in a bulk envelope was accepted.
///
/// The bulk tools answer per item: a rejected write — an unknown task, a
/// transition dropr will not make, an agent that does not hold the claim —
/// comes back as `status: "error"` inside a call that succeeded. An envelope
/// carrying no results accepted nothing.
pub(super) fn accepted(payload: &Value) -> bool {
    payload
        .get("results")
        .and_then(Value::as_array)
        .is_some_and(|results| {
            !results.is_empty()
                && results
                    .iter()
                    .all(|item| item.get("status").and_then(Value::as_str) == Some("ok"))
        })
}

/// What the first rejected item said, for the log line an operator reads.
fn item_refusal(payload: &Value) -> String {
    let message = payload
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|item| item.get("status").and_then(Value::as_str) != Some("ok"))
        .and_then(|item| item.get("error")?.get("message")?.as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| payload.to_string());
    one_line(&message)
}

/// Squashes a server message onto the single line a decision-log reason holds.
fn one_line(message: &str) -> String {
    message
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MESSAGE_LIMIT)
        .collect()
}

#[cfg(test)]
#[path = "writes_tests.rs"]
mod tests;
