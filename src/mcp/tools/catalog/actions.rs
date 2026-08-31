//! Schemas for actions mirrored from TUI row key bindings.

use serde_json::{Value, json};

use super::tool;

const HOST: &str = " Acts on the machine running the robco daemon, not the caller's machine.";

pub(super) fn tools() -> Vec<Value> {
    vec![
        agent_kill(),
        agent_restart(),
        repo_checkout_main(),
        repo_clear_chat(),
        repo_rename(),
        agent_land(),
        daemon_start(),
        daemon_stop(),
        daemon_panic_stop(),
        inbox_dismiss(),
        inbox_dismiss_all(),
        instruct(),
    ]
}

fn definition(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    tool(
        name,
        &format!("{description}{HOST}"),
        json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        }),
    )
}

fn agent_kill() -> Value {
    definition(
        "robco_agent_kill",
        "Kill a worker immediately. Leaves a surviving branch registered unless delete_branch is true. Refuses while the repository is merging; confirm must be true.",
        json!({
            "agent_id": { "type": "string" },
            "force": { "type": "boolean", "default": false },
            "delete_branch": { "type": "boolean", "default": false },
            "confirm": { "type": "boolean", "default": false }
        }),
        &["agent_id", "confirm"],
    )
}

fn agent_restart() -> Value {
    definition(
        "robco_agent_restart",
        "Restart a worker's configured program immediately. Refuses branch-only workers and repositories currently merging.",
        json!({ "agent_id": { "type": "string" } }),
        &["agent_id"],
    )
}

fn repo_checkout_main() -> Value {
    definition(
        "robco_repo_checkout_main",
        "Check out the resolved default branch in a registered repository's primary worktree. Refuses a dirty worktree.",
        repo_path_property(),
        &["repo_path"],
    )
}

fn repo_clear_chat() -> Value {
    definition(
        "robco_repo_clear_chat",
        "Immediately clear a registered repository's main-worktree chat. Refuses an absent or busy session; confirm must be true because history is discarded.",
        json!({
            "repo_path": { "type": "string", "description": "Exact registered absolute path." },
            "confirm": { "type": "boolean", "default": false }
        }),
        &["repo_path", "confirm"],
    )
}

fn repo_rename() -> Value {
    definition(
        "robco_repo_rename",
        "Rename an agent-free registered repository directory and repair linked worktrees immediately.",
        json!({
            "repo_path": { "type": "string", "description": "Exact registered absolute path." },
            "new_name": { "type": "string", "description": "Single plain directory name." }
        }),
        &["repo_path", "new_name"],
    )
}

fn agent_land() -> Value {
    definition(
        "robco_agent_land",
        "Run the TUI landing decision immediately: request a missing PR and queue approval, clean an already merged PR, merge a green open PR, queue a waiting open PR, or report failed checks. confirm must be true.",
        json!({
            "agent_id": { "type": "string" },
            "confirm": { "type": "boolean", "default": false }
        }),
        &["agent_id", "confirm"],
    )
}

fn daemon_start() -> Value {
    definition(
        "robco_daemon_start",
        "Start the installed Overseer daemon service immediately.",
        json!({}),
        &[],
    )
}

fn daemon_stop() -> Value {
    definition(
        "robco_daemon_stop",
        "Durably stop the Overseer daemon immediately.",
        json!({}),
        &[],
    )
}

fn daemon_panic_stop() -> Value {
    definition(
        "robco_daemon_panic_stop",
        "Immediately terminate every Overseer worker and record a panic escalation. confirm must be true.",
        json!({ "confirm": { "type": "boolean", "default": false } }),
        &["confirm"],
    )
}

fn inbox_dismiss() -> Value {
    definition(
        "robco_inbox_dismiss",
        "Dismiss the currently derived Inbox row matching its stable kind and target identity.",
        json!({
            "kind": { "type": "string", "description": "Inbox kind code, currently ESC." },
            "target_id": { "type": "string" }
        }),
        &["kind", "target_id"],
    )
}

fn inbox_dismiss_all() -> Value {
    definition(
        "robco_inbox_dismiss_all",
        "Dismiss every currently derived Inbox row. confirm must be true, matching the TUI's bulk-action confirmation.",
        json!({ "confirm": { "type": "boolean", "default": false } }),
        &["confirm"],
    )
}

fn instruct() -> Value {
    definition(
        "robco_instruct",
        "Send one flattened line and Enter to a named tmux session immediately. Use robco_answer when targeting a registered agent by id.",
        json!({
            "session": { "type": "string" },
            "text": { "type": "string" }
        }),
        &["session", "text"],
    )
}

fn repo_path_property() -> Value {
    json!({
        "repo_path": { "type": "string", "description": "Exact registered absolute path." }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_description_names_the_daemon_machine() {
        for tool in tools() {
            assert!(
                tool["description"]
                    .as_str()
                    .unwrap()
                    .contains("machine running the robco daemon")
            );
        }
    }

    #[test]
    fn every_action_is_registered_with_a_closed_object_schema() {
        let tools = tools();
        let names = tools
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "robco_agent_kill",
                "robco_agent_restart",
                "robco_repo_checkout_main",
                "robco_repo_clear_chat",
                "robco_repo_rename",
                "robco_agent_land",
                "robco_daemon_start",
                "robco_daemon_stop",
                "robco_daemon_panic_stop",
                "robco_inbox_dismiss",
                "robco_inbox_dismiss_all",
                "robco_instruct",
            ]
        );
        for tool in tools {
            assert_eq!(tool["inputSchema"]["type"], "object");
            assert_eq!(tool["inputSchema"]["additionalProperties"], false);
        }
    }
}
