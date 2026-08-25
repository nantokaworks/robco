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
    /// shape, or was rejected on policy grounds — a model formatting slip,
    /// or an action triage's own guards do not allow. The `outcome` and
    /// `reason` above still parsed cleanly, so the caller carries on with
    /// those and only drops the one action it could not use; see [`parse`]'s
    /// doc comment for why neither case is a hard failure.
    pub action_error: Option<String>,
}

/// A `result.json` the daemon cannot fully trust: the top-level JSON is
/// broken, or the reason is blank. There is no usable outcome or reason at
/// all, so the caller has nothing to act on and escalates.
///
/// Two other ways a `result.json` can be unusable are deliberately not
/// hard failures: a *schema* mismatch on the action (a missing or misnamed
/// field, the common shape of a formatting slip), and a *policy* rejection
/// of an action the model was not allowed to take (only
/// `task_status_update`'s own-task and worker-alive guards raise one). Both
/// are recovered in place instead — [`parse`] drops the one unusable action
/// and reports why through [`TriageResult::action_error`], while `outcome`
/// and `reason` still come through — because neither is worth an operator's
/// attention: a model spelling an action wrong, or trying one triage's own
/// guards already caught, is triage's problem to log, not the operator's to
/// act on. See dropr:401 for the schema case, dropr:556 for the policy one.
pub fn parse(
    raw: &[u8],
    own_task: Option<&str>,
    worker_id: &str,
    worker_alive: &dyn Fn(&str) -> bool,
) -> Result<TriageResult, String> {
    let raw: RawResult = serde_json::from_slice(raw).map_err(|error| error.to_string())?;
    if raw.reason.trim().is_empty() {
        return Err("reason must not be blank".into());
    }
    let mut action_error = None;
    let mut action = raw.action.and_then(|mut value| {
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
        // `own_task` is `None` when the case has no known dropr task at all
        // (see `ExceptionCase::dropr_task_id`) — there is then no task lock
        // for the model to legitimately release, so the comparison below
        // rejects every `task_id` it could name (dropr:535).
        let rejection = if own_task != Some(task_id.as_str())
            || !matches!(status.as_str(), "open" | "ready")
        {
            Some("task_status_update may only release this worker's task lock".to_string())
        } else if worker_alive(worker_id) {
            Some("task_status_update refused while the owning worker session is alive".to_string())
        } else {
            None
        };
        if let Some(rejection) = rejection {
            action_error = Some(rejection);
            action = None;
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
