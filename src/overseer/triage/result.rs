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
}

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
    let action = raw
        .action
        .map(|mut value| {
            normalize_tag(&mut value);
            serde_json::from_value(value)
                .map_err(|error| ParseError::RejectedAction(error.to_string()))
        })
        .transpose()?;
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
