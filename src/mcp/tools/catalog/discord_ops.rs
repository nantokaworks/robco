//! Declarations for the read-only Discord commands MCP gained from
//! `Command`: `Tasks`, `Log`, `Diff`, `Help`, `Inbox`. Kept apart from the
//! agent and coordination tools for the same reason `git_ops` is: a distinct
//! source (the Discord ledger/decision log/Inbox) rather than the agent
//! registry.
//!
//! Every other Discord-only command (`Skip`, `Retry`, `TaskCreate`, `Kill`,
//! `Panic`, `Run`) stays Discord-only in this pass — see
//! `overseer::discord::mcp_bridge::mcp_tool_name`'s doc comment for why.

use serde_json::{Value, json};

use super::tool;

pub(super) fn tools() -> Vec<Value> {
    vec![tasks(), log(), diff(), help(), inbox()]
}

/// These four wrap Discord's own reply text (see
/// `overseer::discord::mcp_bridge::text_result`) rather than a typed shape,
/// so none declares an `outputSchema` — the same choice `robco_whoami` and
/// `robco_report` already made for output that has no fixed structure.
fn tasks() -> Value {
    tool(
        "robco_tasks",
        "List the Overseer dispatch ledger's entries and their phase.",
        empty_schema(),
    )
}

fn log() -> Value {
    tool(
        "robco_log",
        "Show the last entries from the Overseer decision log.",
        json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 50,
                    "default": 10
                }
            },
            "additionalProperties": false
        }),
    )
}

fn diff() -> Value {
    tool(
        "robco_diff",
        "Show what an escalated task's pull request changes.",
        json!({
            "type": "object",
            "properties": { "task_id": { "type": "string" } },
            "required": ["task_id"],
            "additionalProperties": false
        }),
    )
}

fn help() -> Value {
    tool(
        "robco_help",
        "List every command robco can be told to do.",
        empty_schema(),
    )
}

fn inbox() -> Value {
    tool(
        "robco_inbox",
        "List the same rows the TUI Inbox shows: each row's move, target, and guidance.",
        empty_schema(),
    )
}

fn empty_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}
