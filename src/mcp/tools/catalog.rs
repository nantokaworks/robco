use serde_json::{Value, json};

mod actions;
mod discord_ops;
mod git_ops;
mod snapshots;

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
            "Create a worker agent in a registered repository: a new git worktree, a branch, \
             and a tmux session running the configured program. If the work belongs to a dropr \
             task, prefer `dropr_task` over a hand-written `title` — it derives the name, claims \
             the task in dropr, and builds the worker's initial prompt from the task body, so \
             none of that has to be done by hand.",
            json!({
                "type": "object",
                "properties": {
                    "repo": {
                        "type": "string",
                        "description": "Registered repository name or absolute path."
                    },
                    "title": {
                        "type": "string",
                        "description": "Names the new branch, worktree, and tmux session — this \
                             is the only source for all three, there is no separate naming \
                             argument. If the work belongs to a dropr task, lead with the task \
                             number: \"538 Launch workers autonomously\", not \"Launch workers \
                             autonomously\" — a tree full of worktrees with no numbers is hard to \
                             read against the task list. Required unless `dropr_task` is set, in \
                             which case it is derived from the task and must be omitted."
                    },
                    "prompt": {
                        "type": "string",
                        "description": "Initial prompt typed into the launched program. Ignored \
                             (and must be omitted) when `dropr_task` is set, since the task's own \
                             body and template build the prompt instead."
                    },
                    "parent_agent_id": {
                        "type": "string",
                        "description": "Id of the agent this worker reports to. Defaults to the \
                             calling agent's own id, or to the Overseer daemon when unset."
                    },
                    "autonomous": {
                        "type": "boolean",
                        "default": false,
                        "description": "Launch with the selected profile's autonomous settings: \
                             the profile's `autonomous_args` (e.g. a permission-bypass flag) are \
                             passed to the launched program, AND the worker's tmux session has \
                             `overseer.worker_env_blocklist` applied to its environment (ambient \
                             credentials such as `*_TOKEN` / `*_API_KEY` are blanked, unless a \
                             session credential channel explicitly names them). false launches an \
                             interactive worker with neither of those."
                    },
                    "dropr_task": {
                        "type": "string",
                        "description": "A dropr task id or display id (`538` or `#538`) to \
                             launch a worker for. Claims the task in dropr, derives `title` as \
                             \"<display_id> <task title>\", and builds the initial prompt from \
                             the task body and the configured worker prompt template. Cannot be \
                             combined with an explicit `title` or `prompt` — the task supplies \
                             both. A launch failure releases the claim."
                    }
                },
                "required": ["repo"],
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
    tools.extend(actions::tools());
    tools.extend(discord_ops::tools());
    tools.extend(snapshots::tools());
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
