//! Dispatch for MCP actions mirrored from TUI row key bindings.

use serde_json::Value;

use super::{ToolResult, parse_args};

mod daemon;
mod inbox;
mod instruct;
mod land;
mod lifecycle;
mod repo;

pub(super) fn dispatch(name: &str, arguments: Option<Value>) -> Option<ToolResult<Value>> {
    Some(match name {
        "robco_agent_kill" => parse_args(arguments).and_then(lifecycle::kill),
        "robco_agent_restart" => parse_args(arguments).and_then(lifecycle::restart),
        "robco_repo_checkout_main" => parse_args(arguments).and_then(repo::checkout_main),
        "robco_repo_clear_chat" => parse_args(arguments).and_then(repo::clear_chat),
        "robco_repo_rename" => parse_args(arguments).and_then(repo::rename),
        "robco_agent_land" => parse_args(arguments).and_then(land::land),
        "robco_daemon_start" => parse_args(arguments).and_then(daemon::start),
        "robco_daemon_stop" => parse_args(arguments).and_then(daemon::stop),
        "robco_daemon_panic_stop" => parse_args(arguments).and_then(daemon::panic_stop),
        "robco_inbox_dismiss" => parse_args(arguments).and_then(inbox::dismiss_one),
        "robco_inbox_dismiss_all" => parse_args(arguments).and_then(inbox::dismiss_all),
        "robco_instruct" => parse_args(arguments).and_then(instruct::instruct),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::mcp::tools::ToolError;

    #[test]
    fn every_action_name_reaches_a_strict_argument_parser() {
        for name in [
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
        ] {
            let error = dispatch(name, Some(json!({ "unexpected": true })))
                .expect("action was not dispatched")
                .unwrap_err();
            assert!(matches!(error, ToolError::InvalidParams(_)), "{name}");
        }
    }
}
