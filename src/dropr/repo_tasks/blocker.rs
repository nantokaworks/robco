//! Looks up a blocked task's reason from its unresolved `blocker` scribble.
//!
//! Split out from the task walk in the parent module because it is a
//! distinct concern answering a distinct question: the walk answers "what
//! tasks exist", this answers "why is this one stuck". The two fail
//! independently — a broken lookup here must not blank out a task the walk
//! already found, so its failures land as problem lines, never as lost rows.

use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::dropr::mcp::{self, ToolOutcome};

use super::DroprTaskFetch;

/// Blocked tasks one fetch will look up a reason for. Mirrors the parent
/// module's `SUBTREE_QUERY_LIMIT`: a board with more blocked tasks than this
/// gets a reported shortfall rather than silently missing reasons.
const BLOCKER_QUERY_LIMIT: usize = 8;

/// A `scribble_list` row. The query that reads this already filters to
/// `kind: "blocker"`, so `body` is the only field the lookup needs.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct ScribbleRow {
    #[serde(default)]
    body: String,
}

/// Answers to one batch of `scribble_list` calls. Same shape as the parent
/// module's `Answers`, for the blocked-reason lookups.
pub(super) type ScribbleAnswers = Result<Vec<Result<Vec<ScribbleRow>, String>>, String>;

/// Runs one batch of `scribble_list` calls over `dropr mcp-stdio`, mirroring
/// the parent module's `mcp_asker` for the blocked-reason lookups.
pub(super) fn mcp_scribble_asker(arguments: Vec<Value>, timeout: Duration) -> ScribbleAnswers {
    let calls = arguments
        .into_iter()
        .map(|arguments| ("scribble_list", arguments))
        .collect::<Vec<_>>();
    let outcomes = mcp::call_tools(&calls, timeout).ok_or_else(|| {
        "dropr mcp-stdio gave no answer — the binary is missing, it died, or it timed out"
            .to_string()
    })?;
    Ok(outcomes.into_iter().map(scribble_rows).collect())
}

fn scribble_rows(outcome: ToolOutcome) -> Result<Vec<ScribbleRow>, String> {
    match outcome {
        ToolOutcome::Ok(payload) => scribbles(&payload)
            .ok_or_else(|| "scribble_list answered in a shape robco cannot read".to_string()),
        ToolOutcome::Refused(message) => {
            Err(format!("dropr refused: {}", super::one_line(&message)))
        }
    }
}

fn scribbles(payload: &Value) -> Option<Vec<ScribbleRow>> {
    let scribbles = payload.get("scribbles")?.as_array()?;
    Some(
        scribbles
            .iter()
            .filter_map(|scribble| serde_json::from_value(scribble.clone()).ok())
            .collect(),
    )
}

/// Fills in each blocked task's `blocked_reason` from its unresolved
/// `blocker` scribble, spending whatever is left of the shared fetch budget.
///
/// Runs after the task walk has already settled: a row a task fetch found is
/// worth more than the reason behind it, so a failure here becomes a problem
/// line rather than a reason to blank out tasks the walk already answered.
pub(super) fn attach_blocked_reasons<G>(
    fetch: &mut DroprTaskFetch,
    workspace_id: &str,
    deadline: Instant,
    mut ask: G,
) where
    G: FnMut(Vec<Value>, Duration) -> ScribbleAnswers,
{
    if !fetch.answered {
        return;
    }
    let blocked = fetch
        .tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| task.status == "blocked")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if blocked.is_empty() {
        return;
    }
    if let Some(skipped) = blocked
        .len()
        .checked_sub(BLOCKER_QUERY_LIMIT)
        .filter(|n| *n > 0)
    {
        fetch.problems.push(format!(
            "blocker reason of {skipped} further blocked task(s) was not asked for"
        ));
    }
    let walked = &blocked[..blocked.len().min(BLOCKER_QUERY_LIMIT)];
    let queries = walked
        .iter()
        .map(|&index| blocker_query(workspace_id, &fetch.tasks[index].display_id))
        .collect();
    match super::ask_within(&mut ask, queries, deadline) {
        Ok(answers) => {
            for (&index, answer) in walked.iter().zip(answers) {
                match answer {
                    Ok(rows) => {
                        fetch.tasks[index].blocked_reason =
                            rows.into_iter().next().map(|row| row.body);
                    }
                    Err(problem) => fetch.problems.push(format!(
                        "blocker reason of {}: {problem}",
                        fetch.tasks[index].display_id
                    )),
                }
            }
        }
        Err(problem) => fetch.problems.push(format!(
            "blocker reason of {} blocked task(s): {problem}",
            walked.len()
        )),
    }
}

/// The scribble lookup for one blocked task's reason. `display_id` is enough
/// to target the task because `workspace_id` travels alongside — see
/// `scribble_list`'s display-id-accepting contract. Ancestors are excluded so
/// a parent's own blocker scribble cannot leak onto a child that carries none
/// of its own.
fn blocker_query(workspace_id: &str, display_id: &str) -> Value {
    json!({
        "workspace_id": workspace_id,
        "task_id": display_id,
        "kind": "blocker",
        "unresolved_only": true,
        "include_ancestors": false,
        "limit": 1,
    })
}

#[cfg(test)]
#[path = "blocker_tests.rs"]
mod tests;
