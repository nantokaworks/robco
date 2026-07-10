use serde_json::{Value, json};

pub fn list_tools() -> Value {
    json!([
        tool(
            "robco_whoami",
            "Report the calling agent's inherited identity.",
            empty_schema()
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
            })
        ),
        tool(
            "robco_agent_list",
            "List repos and agents with live status.",
            empty_schema()
        ),
        tool(
            "robco_agent_status",
            "Get one agent's live status.",
            json!({
                "type": "object",
                "properties": { "agent_id": { "type": "string" } },
                "required": ["agent_id"],
                "additionalProperties": false
            })
        ),
        tool(
            "robco_question_list",
            "List agents awaiting confirmation prompts.",
            empty_schema()
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
            })
        ),
        tool(
            "robco_approve",
            "Approve an agent confirmation prompt.",
            json!({
                "type": "object",
                "properties": { "agent_id": { "type": "string" } },
                "required": ["agent_id"],
                "additionalProperties": false
            })
        )
    ])
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
