//! Resolving a bare task reference (`"538"`, `"#538"`, or a nanoid) to the
//! full task row a launch needs — title, child_count — before a CLI/MCP
//! driven spawn (dropr:540) ever attempts to claim it. TUI callers skip
//! this: they already hold the row from their own `task_list`-backed cache.

use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    DroprTaskCandidate,
    mcp::{ToolOutcome, call_tool},
};

/// What a task-reference lookup came back with.
pub enum TaskLookup {
    Found(Box<DroprTaskCandidate>),
    /// dropr answered that no task matches the reference.
    NotFound,
    /// The call never reached a verdict, or answered in a shape this cannot
    /// read.
    Unavailable,
}

#[derive(Deserialize)]
struct TaskListPayload {
    #[serde(default)]
    tasks: Vec<DroprTaskCandidate>,
}

pub fn lookup_task(workspace_id: &str, task_ref: &str, timeout: Duration) -> TaskLookup {
    lookup_task_with(workspace_id, task_ref, timeout, |arguments, timeout| {
        call_tool("task_list", arguments, timeout)
    })
}

fn lookup_task_with<F>(workspace_id: &str, task_ref: &str, timeout: Duration, ask: F) -> TaskLookup
where
    F: FnOnce(Value, Duration) -> Option<ToolOutcome>,
{
    let arguments = json!({
        "workspace_id": workspace_id,
        "task_id": task_ref,
        "limit": 1,
    });
    match ask(arguments, timeout) {
        Some(ToolOutcome::Ok(payload)) => {
            match serde_json::from_value::<TaskListPayload>(payload)
                .ok()
                .and_then(|payload| payload.tasks.into_iter().next())
            {
                Some(candidate) => TaskLookup::Found(Box::new(candidate)),
                None => TaskLookup::NotFound,
            }
        }
        Some(ToolOutcome::Refused(message)) if is_not_found(&message) => TaskLookup::NotFound,
        Some(ToolOutcome::Refused(_)) => TaskLookup::Unavailable,
        None => TaskLookup::Unavailable,
    }
}

/// dropr answers a task reference that resolves to nothing with a plain-text
/// `"task not found: <ref>"` refusal — confirmed live against a real
/// workspace before writing this — distinct from every other refusal shape,
/// which arrives as JSON (`{"code": ..., "reason": ...}`).
fn is_not_found(message: &str) -> bool {
    message.trim_start().starts_with("task not found")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate_row(display_id: &str) -> Value {
        json!({
            "global_display_id": display_id,
            "id": "task-nanoid",
            "title": "Do the thing",
            "child_count": 0,
        })
    }

    #[test]
    fn a_found_task_carries_its_full_row() {
        let payload = json!({ "tasks": [candidate_row("#538")] });
        let result = lookup_task_with("ws-1", "#538", Duration::from_secs(1), |_, _| {
            Some(ToolOutcome::Ok(payload))
        });
        let TaskLookup::Found(found) = result else {
            panic!("expected Found");
        };
        assert_eq!(found.display_id, "#538");
        assert_eq!(found.id, "task-nanoid");
    }

    #[test]
    fn a_plain_text_not_found_refusal_is_recognised() {
        let result = lookup_task_with("ws-1", "#99999", Duration::from_secs(1), |_, _| {
            Some(ToolOutcome::Refused("task not found: #99999".to_string()))
        });
        assert!(matches!(result, TaskLookup::NotFound));
    }

    #[test]
    fn an_empty_task_list_is_also_not_found() {
        let result = lookup_task_with("ws-1", "#538", Duration::from_secs(1), |_, _| {
            Some(ToolOutcome::Ok(json!({ "tasks": [] })))
        });
        assert!(matches!(result, TaskLookup::NotFound));
    }

    #[test]
    fn any_other_refusal_is_unavailable_not_not_found() {
        let result = lookup_task_with("ws-1", "#538", Duration::from_secs(1), |_, _| {
            Some(ToolOutcome::Refused("dropr login required".to_string()))
        });
        assert!(matches!(result, TaskLookup::Unavailable));
    }

    #[test]
    fn no_answer_at_all_is_unavailable() {
        let result = lookup_task_with("ws-1", "#538", Duration::from_secs(1), |_, _| None);
        assert!(matches!(result, TaskLookup::Unavailable));
    }
}
