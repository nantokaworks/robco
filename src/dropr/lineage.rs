//! Parent/child task_list lookups `overseer::dispatch` needs and `dropr task
//! ready` does not carry: which ready candidates are subtasks and of whom
//! (dropr:yD5Gf6TX23VMvuSLFsmvO defect 2), and which subtasks a dispatched
//! parent task covers (dropr:yD5Gf6TX23VMvuSLFsmvO defect 1).

use std::{collections::HashMap, time::Duration};

use serde::Deserialize;
use serde_json::json;

use super::mcp::{ToolOutcome, call_tool};

#[derive(Debug, Clone, Default, Deserialize)]
struct LineageRow {
    #[serde(default)]
    id: String,
    #[serde(default, alias = "global_display_id")]
    display_id: String,
    #[serde(default)]
    parent_task_id: Option<String>,
}

#[derive(Deserialize)]
struct TaskListPayload {
    #[serde(default)]
    tasks: Vec<LineageRow>,
}

fn list(arguments: serde_json::Value, timeout: Duration) -> Vec<LineageRow> {
    let Some(ToolOutcome::Ok(payload)) = call_tool("task_list", arguments, timeout) else {
        return Vec::new();
    };
    serde_json::from_value::<TaskListPayload>(payload)
        .map(|parsed| parsed.tasks)
        .unwrap_or_default()
}

/// Each requested id's `parent_task_id`, batched into one `task_list` call
/// rather than one round-trip per candidate. An id absent from the result
/// means dropr's answer did not include a row for it — the whole call failed,
/// or the task no longer exists — and callers must treat that as unknown, not
/// as "no parent".
pub fn fetch_parents(
    workspace_id: &str,
    task_ids: &[String],
    timeout: Duration,
) -> HashMap<String, Option<String>> {
    if task_ids.is_empty() {
        return HashMap::new();
    }
    list(
        json!({
            "workspace_id": workspace_id,
            "task_ids": task_ids,
            "limit": task_ids.len().clamp(1, 100),
        }),
        timeout,
    )
    .into_iter()
    .filter(|row| !row.id.is_empty())
    .map(|row| (row.id, row.parent_task_id))
    .collect()
}

/// One subtask's identifiers, as `worker_prompt` needs them for its
/// `Close Dropr:` lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subtask {
    pub display_id: String,
}

/// Every descendant of `parent_task_id`, for the dispatch prompt this parent's
/// worker is handed. Empty (rather than an error type) when the lookup fails
/// or the task is genuinely a leaf — `worker.rs` reads that the same way
/// either way: fall back to the childless prompt shape.
pub fn fetch_subtasks(workspace_id: &str, parent_task_id: &str, timeout: Duration) -> Vec<Subtask> {
    list(
        json!({
            "workspace_id": workspace_id,
            "parent_task_id": parent_task_id,
            "include_descendants": true,
            "limit": 100,
        }),
        timeout,
    )
    .into_iter()
    .filter(|row| !row.display_id.is_empty())
    .map(|row| Subtask {
        display_id: row.display_id,
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_parents_of_an_empty_request_makes_no_call() {
        assert_eq!(
            fetch_parents("workspace", &[], Duration::from_secs(1)),
            HashMap::new()
        );
    }
}
