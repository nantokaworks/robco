use serde_json::{Value, json};

use super::{empty_schema, tool, tool_with_output};

pub(super) fn tools() -> Vec<Value> {
    vec![
        tool(
            "robco_pane_capture",
            "Resize and capture a tmux pane as ANSI text; output is capped at 256 KiB.",
            json!({
                "type": "object",
                "properties": {
                    "session": { "type": "string" },
                    "width": { "type": "integer", "minimum": 1, "maximum": 65535 },
                    "height": { "type": "integer", "minimum": 1, "maximum": 65535 },
                    "offset": {
                        "type": "integer",
                        "minimum": 0,
                        "maximum": 65535,
                        "default": 0,
                        "description": "Lines back from the live edge."
                    }
                },
                "required": ["session", "width", "height"],
                "additionalProperties": false
            }),
        ),
        tool_with_output(
            "robco_discovery_snapshot",
            "Discover repositories, agents, child worktrees, orphan sessions, and subagent counts.",
            empty_schema(),
            open_object_schema(),
        ),
        tool_with_output(
            "robco_overseer_snapshot",
            "Read the consolidated Overseer state needed to render its remote UI panes.",
            empty_schema(),
            open_object_schema(),
        ),
    ]
}

fn open_object_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": true
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_schema_requires_dimensions_and_documents_cap() {
        let tools = tools();
        let pane = tools
            .iter()
            .find(|tool| tool["name"] == "robco_pane_capture")
            .unwrap();
        assert_eq!(
            pane["inputSchema"]["required"],
            json!(["session", "width", "height"])
        );
        assert!(pane["description"].as_str().unwrap().contains("256 KiB"));
    }
}
