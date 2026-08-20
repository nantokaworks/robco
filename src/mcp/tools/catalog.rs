use serde_json::{Value, json};

mod discord_ops;
mod git_ops;

pub fn list_tools() -> Value {
    let mut tools = vec![
        tool(
            "robco_whoami",
            "Report the calling agent's inherited identity.",
            empty_schema(),
        ),
        tool(
            "robco_report",
            "Send a labeled report to a controller agent when it is safe to interrupt.",
            json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "Report text; control characters are collapsed to spaces and the delivered report is a single line."
                    },
                    "target_agent_id": { "type": "string" }
                },
                "required": ["message"],
                "additionalProperties": false
            }),
        ),
        tool_with_output(
            "robco_overseer_policy",
            "Read the Overseer daemon's current local policy and health.",
            empty_schema(),
            json!({
                "type": "object",
                "properties": {
                    "auto_merge": { "type": "boolean" },
                    "daemon_alive": { "type": "boolean" }
                },
                "required": ["auto_merge", "daemon_alive"],
                "additionalProperties": false
            }),
        ),
        tool_with_output(
            "robco_agent_list",
            "List repos and agents with live status.",
            empty_schema(),
            agent_list_schema(),
        ),
        tool_with_output(
            "robco_agent_create",
            "Create a worker agent in a registered repository.",
            json!({
                "type": "object",
                "properties": {
                    "repo": { "type": "string" },
                    "title": { "type": "string" },
                    "prompt": { "type": "string" },
                    "parent_agent_id": { "type": "string" },
                    "autonomous": { "type": "boolean", "default": false }
                },
                "required": ["repo", "title"],
                "additionalProperties": false
            }),
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "branch": { "type": "string" },
                    "worktree_path": { "type": "string" },
                    "tmux_session": { "type": "string" }
                },
                "required": ["id", "branch", "worktree_path", "tmux_session"],
                "additionalProperties": false
            }),
        ),
        tool_with_output(
            "robco_agent_status",
            "Get one agent's live status.",
            json!({
                "type": "object",
                "properties": { "agent_id": { "type": "string" } },
                "required": ["agent_id"],
                "additionalProperties": false
            }),
            agent_status_schema(),
        ),
        tool(
            "robco_question_list",
            "List agents awaiting confirmation prompts.",
            empty_schema(),
        ),
        tool(
            "robco_answer",
            "Send text and Enter to an agent session.",
            json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "text": { "type": "string" }
                },
                "required": ["agent_id", "text"],
                "additionalProperties": false
            }),
        ),
        tool_with_output(
            "robco_approve",
            "Approve an agent confirmation prompt. If the agent has no live session \
             (e.g. it was killed), instead requests a merge for its pull request, for \
             the merge pass to pick up on its next tick — this fallback requires \
             confirm: true, the same way robco_merge does, since it drives a merge \
             through the daemon rather than answer a live prompt.",
            json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "confirm": {
                        "type": "boolean",
                        "default": false,
                        "description": "Required only when the agent has no live session; \
                                         ignored otherwise."
                    }
                },
                "required": ["agent_id"],
                "additionalProperties": false
            }),
            json!({
                "type": "object",
                "properties": {
                    "ok": { "type": "boolean" },
                    "mode": { "type": "string", "enum": ["session", "operator_override"] },
                    "target": { "type": "string" }
                },
                "required": ["ok", "mode"],
                "additionalProperties": false
            }),
        ),
    ];
    tools.extend(git_ops::tools());
    tools.extend(discord_ops::tools());
    Value::Array(tools)
}

fn empty_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema
    })
}

fn tool_with_output(
    name: &str,
    description: &str,
    input_schema: Value,
    output_schema: Value,
) -> Value {
    let mut definition = tool(name, description, input_schema);
    definition["outputSchema"] = output_schema;
    definition
}

fn activity_properties() -> Value {
    json!({
        "tracked_command": { "type": ["string", "null"] },
        "subagents_active": { "type": "integer", "minimum": 0 }
    })
}

fn agent_list_schema() -> Value {
    let mut properties = json!({
        "id": { "type": "string" },
        "title": { "type": "string" },
        "branch": { "type": "string" },
        "tmux_session": { "type": "string" },
        "status": { "type": "string" },
        "worktree_missing": { "type": "boolean" }
    });
    properties
        .as_object_mut()
        .unwrap()
        .extend(activity_properties().as_object().unwrap().clone());
    json!({
        "type": "object",
        "properties": {
            "repos": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "path": { "type": "string" },
                        "agents": { "type": "array", "items": {
                            "type": "object", "properties": properties,
                            "required": ["id", "title", "branch", "tmux_session", "status",
                                "worktree_missing",
                                "tracked_command", "subagents_active"],
                            "additionalProperties": false
                        }}
                    },
                    "required": ["name", "path", "agents"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["repos"],
        "additionalProperties": false
    })
}

fn agent_status_schema() -> Value {
    let mut properties = json!({
        "agent_id": { "type": "string" },
        "title": { "type": "string" },
        "tmux_session": { "type": "string" },
        "status": { "type": "string" },
        "worktree_missing": { "type": "boolean" },
        "awaiting_confirmation": { "type": "boolean" },
        "prompt": { "type": "string" }
    });
    properties
        .as_object_mut()
        .unwrap()
        .extend(activity_properties().as_object().unwrap().clone());
    json!({
        "type": "object",
        "properties": properties,
        "required": ["agent_id", "title", "tmux_session", "status", "worktree_missing",
            "tracked_command", "subagents_active", "awaiting_confirmation", "prompt"],
        "additionalProperties": false
    })
}
