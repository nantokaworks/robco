use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Resolved,
    Skip,
    Escalate,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "name", rename_all = "snake_case")]
pub enum TriageAction {
    RobcoAgentStatus {
        agent_id: String,
    },
    RobcoAnswer {
        agent_id: String,
        text: String,
    },
    RobcoApprove {
        agent_id: String,
    },
    DroprScribbleCreate {
        task_id: String,
        content: String,
    },
    DroprTaskStatusUpdate {
        task_id: String,
        status: String,
    },
    RobcoAgentCreate {
        repo: String,
        title: String,
        prompt: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
struct RawResult {
    outcome: Outcome,
    #[serde(default)]
    action: Option<Value>,
    reason: String,
}

pub(super) fn is_complete(raw: &[u8]) -> bool {
    serde_json::from_slice::<RawResult>(raw).is_ok()
}

#[derive(Debug, PartialEq, Eq)]
pub struct TriageResult {
    pub outcome: Outcome,
    pub action: Option<TriageAction>,
    pub reason: String,
    /// Set when `action` failed to deserialize against a known action
    /// shape — a model formatting slip, such as a missing field — rather
    /// than being rejected on its own merits. The `outcome` and `reason`
    /// above still parsed cleanly, so the caller carries on with those and
    /// only drops the one action it could not read; see `ParseError`'s doc
    /// comment for why this is not a hard failure.
    pub action_error: Option<String>,
}

/// A `result.json` the daemon cannot fully trust.
///
/// `Malformed` means there is no usable outcome or reason at all — the top
/// level JSON itself is broken, or the reason is blank — so the caller has
/// nothing to act on and escalates. `RejectedAction` is narrower: it is a
/// *policy* rejection of an action the model was not allowed to take (only
/// `task_status_update`'s own-task and worker-alive guards raise it), which
/// stays a hard failure because a model attempting a disallowed action is
/// worth an operator's attention. A *schema* mismatch on the action — a
/// missing or misnamed field, the common shape of a formatting slip — is
/// deliberately not a variant here: [`parse`] recovers from it in place and
/// reports it through [`TriageResult::action_error`] instead, so a session
/// whose outcome and reason parsed fine is not thrown away over an action it
/// tried and failed to spell correctly. See dropr:401.
#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    Malformed(String),
    RejectedAction(String),
}

pub fn parse(
    raw: &[u8],
    own_task: &str,
    worker_id: &str,
    worker_alive: &dyn Fn(&str) -> bool,
) -> Result<TriageResult, ParseError> {
    let raw: RawResult =
        serde_json::from_slice(raw).map_err(|error| ParseError::Malformed(error.to_string()))?;
    if raw.reason.trim().is_empty() {
        return Err(ParseError::Malformed("reason must not be blank".into()));
    }
    let mut action_error = None;
    let action = raw.action.and_then(|mut value| {
        normalize_tag(&mut value);
        let name = value
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| "<unnamed>".into());
        match serde_json::from_value(value) {
            Ok(action) => Some(action),
            Err(error) => {
                action_error = Some(format!("{name}: {error}"));
                None
            }
        }
    });
    if let Some(TriageAction::DroprTaskStatusUpdate { task_id, status }) = &action {
        if task_id != own_task || !matches!(status.as_str(), "open" | "ready") {
            return Err(ParseError::RejectedAction(
                "task_status_update may only release this worker's task lock".into(),
            ));
        }
        if worker_alive(worker_id) {
            return Err(ParseError::RejectedAction(
                "task_status_update refused while the owning worker session is alive".into(),
            ));
        }
    }
    Ok(TriageResult {
        outcome: raw.outcome,
        action,
        reason: raw.reason,
        action_error,
    })
}

fn normalize_tag(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    if let Some(Value::Object(arguments)) = object.remove("arguments") {
        object.extend(arguments);
    }
    if !object.contains_key("name")
        && let Some(kind) = object.remove("type").or_else(|| object.remove("kind"))
    {
        object.insert("name".into(), kind);
    }
}

#[cfg(test)]
#[path = "result_tests.rs"]
mod tests;
