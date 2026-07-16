use super::commands::Command;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct RawResult {
    reply: String,
    #[serde(default)]
    actions: Vec<Value>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct OpsResult {
    pub reply: String,
    pub actions: Vec<Result<Command, String>>,
}

pub(super) fn is_complete(raw: &[u8]) -> bool {
    serde_json::from_slice::<RawResult>(raw).is_ok()
}

pub(super) fn parse(raw: &[u8]) -> Result<OpsResult, String> {
    let result: RawResult = serde_json::from_slice(raw).map_err(|error| error.to_string())?;
    if result.reply.trim().is_empty() {
        return Err("reply must not be blank".into());
    }
    Ok(OpsResult {
        reply: result.reply,
        actions: result.actions.into_iter().map(parse_action).collect(),
    })
}

fn parse_action(mut value: Value) -> Result<Command, String> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| "action must be an object".to_string())?;
    if let Some(Value::Object(arguments)) = object.remove("arguments") {
        object.extend(arguments);
    }
    let name = object
        .remove("name")
        .or_else(|| object.remove("type"))
        .and_then(|value| value.as_str().map(str::to_string))
        .ok_or_else(|| "action name is missing".to_string())?;
    let text = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .ok_or_else(|| format!("{name}.{key} is missing"))
    };
    let command = match name.as_str() {
        "status" | "robco_status" => Command::Status,
        "workers" | "robco_workers" => Command::Workers,
        "tasks" | "robco_tasks" => Command::Tasks,
        "log" | "robco_log" => Command::Log(
            object
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(10)
                .min(50) as usize,
        ),
        "dispatch" | "robco_dispatch" => Command::Dispatch(bool_arg(object, "enabled")?),
        "automerge" | "robco_automerge" => {
            let enabled = bool_arg(object, "enabled")?;
            if enabled {
                return Err("automerge on is forbidden from Discord".into());
            }
            Command::AutoMerge(false)
        }
        "skip" | "dropr_task_skip" => Command::Skip(text("task_id")?),
        "retry" | "dropr_task_retry" => Command::Retry(text("task_id")?),
        "robco_answer" => Command::Answer {
            agent: text("agent_id")?,
            text: text("text")?,
        },
        "robco_approve" => Command::Approve(text("agent_id")?),
        "robco_kill" => Command::Kill(text("agent_id")?),
        "panic" | "robco_panic" => Command::Panic,
        _ => {
            return Err(format!(
                "action `{name}` is outside the Discord command set"
            ));
        }
    };
    Ok(command)
}

fn bool_arg(object: &serde_json::Map<String, Value>, key: &str) -> Result<bool, String> {
    object
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("{key} must be boolean"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_and_automerge_on() {
        let unknown = br#"{"reply":"no","actions":[{"name":"shell","cmd":"oops"}]}"#;
        assert!(parse(unknown).unwrap().actions[0].is_err());
        let merge = br#"{"reply":"no","actions":[{"name":"automerge","enabled":true}]}"#;
        assert!(parse(merge).unwrap().actions[0].is_err());
    }
}
