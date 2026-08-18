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
        "dropr_task_create" => Command::TaskCreate {
            repo: text("repo")?,
            title: text("title")?,
            description: object
                .get("description")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string),
        },
        "robco_answer" => Command::Answer {
            agent: text("agent_id")?,
            text: text("text")?,
        },
        "robco_approve" => Command::Approve(text("agent_id")?),
        "robco_kill" => Command::Kill(text("agent_id")?),
        "panic" | "robco_panic" => Command::Panic,
        "merge" | "robco_merge" => Command::Merge(text("task_id")?),
        "diff" | "robco_diff" => Command::Diff(text("task_id")?),
        "help" | "robco_help" => Command::Help,
        "run" | "robco_run" => Command::Run(text("task_id")?),
        "inbox" | "robco_inbox" => Command::Inbox,
        "whoami" | "robco_whoami" => Command::Whoami,
        "report" | "robco_report" => Command::Report {
            message: text("message")?,
            target_agent_id: object
                .get("target_agent_id")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string),
        },
        "agentcreate" | "robco_agent_create" => Command::AgentCreate {
            repo: text("repo")?,
            title: text("title")?,
            prompt: object
                .get("prompt")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string),
            parent_agent_id: object
                .get("parent_agent_id")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string),
            autonomous: object
                .get("autonomous")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        },
        "questionlist" | "robco_question_list" => Command::QuestionList,
        "prstatus" | "robco_pr_status" => Command::PrStatus(text("agent_id")?),
        "prrequest" | "robco_pr_request" => Command::PrRequest {
            agent: text("agent_id")?,
            prompt: object
                .get("prompt")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string),
        },
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

    #[test]
    fn parses_task_create_with_optional_description() {
        let raw = br#"{"reply":"ok","actions":[
            {"name":"dropr_task_create","repo":"robco","title":"fix the thing"},
            {"name":"dropr_task_create","repo":"robco","title":"fix the thing","description":"details"}
        ]}"#;
        let actions = parse(raw).unwrap().actions;
        assert_eq!(
            actions[0],
            Ok(Command::TaskCreate {
                repo: "robco".into(),
                title: "fix the thing".into(),
                description: None,
            })
        );
        assert_eq!(
            actions[1],
            Ok(Command::TaskCreate {
                repo: "robco".into(),
                title: "fix the thing".into(),
                description: Some("details".into()),
            })
        );
    }

    #[test]
    fn parses_merge_and_diff() {
        let raw = br#"{"reply":"ok","actions":[
            {"name":"merge","task_id":"task-448"},
            {"name":"robco_diff","task_id":"task-448"}
        ]}"#;
        let actions = parse(raw).unwrap().actions;
        assert_eq!(actions[0], Ok(Command::Merge("task-448".into())));
        assert_eq!(actions[1], Ok(Command::Diff("task-448".into())));
    }

    #[test]
    fn parses_help() {
        let raw = br#"{"reply":"ok","actions":[{"name":"robco_help"}]}"#;
        assert_eq!(parse(raw).unwrap().actions, [Ok(Command::Help)]);
    }

    #[test]
    fn rejects_task_create_missing_required_fields() {
        let missing_repo =
            br#"{"reply":"no","actions":[{"name":"dropr_task_create","title":"x"}]}"#;
        assert!(parse(missing_repo).unwrap().actions[0].is_err());
        let missing_title =
            br#"{"reply":"no","actions":[{"name":"dropr_task_create","repo":"robco"}]}"#;
        assert!(parse(missing_title).unwrap().actions[0].is_err());
    }

    #[test]
    fn parses_the_six_commands_shared_with_mcp() {
        let raw = br#"{"reply":"ok","actions":[
            {"name":"robco_whoami"},
            {"name":"robco_report","message":"turn done"},
            {"name":"robco_question_list"},
            {"name":"robco_pr_status","agent_id":"a1"},
            {"name":"robco_pr_request","agent_id":"a1"},
            {"name":"robco_agent_create","repo":"robco","title":"fix the thing"}
        ]}"#;
        let actions = parse(raw).unwrap().actions;
        assert_eq!(actions[0], Ok(Command::Whoami));
        assert_eq!(
            actions[1],
            Ok(Command::Report {
                message: "turn done".into(),
                target_agent_id: None,
            })
        );
        assert_eq!(actions[2], Ok(Command::QuestionList));
        assert_eq!(actions[3], Ok(Command::PrStatus("a1".into())));
        assert_eq!(
            actions[4],
            Ok(Command::PrRequest {
                agent: "a1".into(),
                prompt: None,
            })
        );
        assert_eq!(
            actions[5],
            Ok(Command::AgentCreate {
                repo: "robco".into(),
                title: "fix the thing".into(),
                prompt: None,
                parent_agent_id: None,
                autonomous: false,
            })
        );
    }

    #[test]
    fn report_carries_an_explicit_target_agent_id() {
        let raw = br#"{"reply":"ok","actions":[
            {"name":"report","message":"hi","target_agent_id":"a1"}
        ]}"#;
        assert_eq!(
            parse(raw).unwrap().actions[0],
            Ok(Command::Report {
                message: "hi".into(),
                target_agent_id: Some("a1".into()),
            })
        );
    }

    #[test]
    fn rejects_the_six_shared_commands_missing_required_fields() {
        for raw in [
            br#"{"reply":"no","actions":[{"name":"robco_report"}]}"# as &[u8],
            br#"{"reply":"no","actions":[{"name":"robco_pr_status"}]}"#,
            br#"{"reply":"no","actions":[{"name":"robco_pr_request"}]}"#,
            br#"{"reply":"no","actions":[{"name":"robco_agent_create","title":"x"}]}"#,
            br#"{"reply":"no","actions":[{"name":"robco_agent_create","repo":"robco"}]}"#,
        ] {
            assert!(parse(raw).unwrap().actions[0].is_err());
        }
    }
}
