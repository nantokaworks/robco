//! Bridges `Command` execution into MCP's tool-result shape, and declares
//! which MCP tool (if any) reaches each `Command` variant.
//!
//! `mcp_tool_name`'s match has no wildcard arm, so a variant added to
//! `Command` without a decision here fails to compile — the same discipline
//! `command_gate` and `help::describe` already use. This is what "a test
//! fails when a variant is reachable from only one surface" (dropr:464)
//! turns into for the variants that stay Discord-only on purpose: a
//! compile-time forcing function plus the test below, which checks every
//! `Some` name actually exists in `crate::mcp::tools::list_tools`, instead
//! of an assertion that would just be wrong for those variants.
//!
//! See the dropr:463 parent task's decision scribble for why Skip, Retry,
//! TaskCreate, Kill, and Panic are `None` here: they are all impactful,
//! confirmation-gated commands whose only safety net today is Discord's
//! CONFIRM-nonce round trip, which lives in `handler::Handler`'s message
//! flow rather than in `actions::execute`'s business logic — giving them an
//! MCP tool without an equivalent gate would be a permission-boundary
//! regression the task explicitly rules out. Status, AutoMerge,
//! and Workers are also `None`: the parent task's own gap count never
//! listed them, and `robco_overseer_policy` / `robco_agent_list` already
//! serve the same operator need in a different shape.

use std::sync::mpsc;

use crate::mcp::tools::{ToolError, ToolResult};

use super::{actions, commands::Command};

const MCP_CALLER: &str = "mcp";

/// Runs `command` through the same [`actions::execute`] Discord's
/// `SystemExecutor` uses, and wraps the reply as an MCP tool result.
///
/// Only safe for commands with no ledger-channel side effect: the throwaway
/// sender's receiver is dropped unread, so a command that actually sends on
/// it would silently lose that message. Every caller of this function is one
/// of the read-only commands `discord_ops::tools()` declares.
pub(crate) fn text_result(command: &Command) -> ToolResult<serde_json::Value> {
    let (ledger_requests, _unread) = mpsc::channel();
    let result = actions::execute(command, MCP_CALLER, &ledger_requests);
    let outcome = match &result {
        Ok(_) => "succeeded".to_string(),
        Err(error) => format!("failed/refused: {error}"),
    };
    if let Err(error) = actions::audit(command, MCP_CALLER, "mcp", &outcome) {
        eprintln!("overseer: failed to audit MCP command: {error}");
    }
    result
        .map(|text| serde_json::json!({ "result": text }))
        .map_err(|error| ToolError::Execution(error.to_string()))
}

pub(crate) fn mcp_tool_name(command: &Command) -> Option<&'static str> {
    match command {
        Command::Status => None,
        Command::AutoMerge(_) => None,
        Command::Workers => None,
        Command::Tasks => Some("robco_tasks"),
        Command::Skip(_) => None,
        Command::Retry(_) => None,
        Command::TaskCreate { .. } => None,
        Command::Answer { .. } => Some("robco_answer"),
        Command::Approve(_) => Some("robco_approve"),
        Command::Kill(_) => None,
        Command::Log(_) => Some("robco_log"),
        Command::Panic => None,
        Command::Merge(_) => Some("robco_merge"),
        Command::Diff(_) => Some("robco_diff"),
        Command::Help => Some("robco_help"),
        Command::Whoami => Some("robco_whoami"),
        Command::Report { .. } => Some("robco_report"),
        Command::AgentCreate { .. } => Some("robco_agent_create"),
        Command::QuestionList => Some("robco_question_list"),
        Command::PrStatus(_) => Some("robco_pr_status"),
        Command::PrRequest { .. } => Some("robco_pr_request"),
        // `Run` spawns a worker exactly like `Skip`/`Retry`/`TaskCreate`/`Kill`/
        // `Panic` change something real, so it stays in the same
        // confirmation-gated, Discord-only group as those five — see dropr:463's
        // decision scribble.
        Command::Run(_) => None,
        Command::Inbox => Some("robco_inbox"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::list_tools;
    use crate::overseer::discord::help::samples;

    #[test]
    fn every_declared_mcp_tool_name_is_a_real_tool() {
        let tools = list_tools();
        let names: Vec<&str> = tools
            .as_array()
            .expect("list_tools returns an array")
            .iter()
            .map(|tool| tool["name"].as_str().expect("tool has a name"))
            .collect();
        for command in samples() {
            if let Some(name) = mcp_tool_name(&command) {
                assert!(
                    names.contains(&name),
                    "declared but missing MCP tool: {name}"
                );
            }
        }
    }

    #[test]
    fn read_only_commands_reach_mcp_through_the_shared_executor() {
        assert_eq!(
            text_result(&Command::Help).unwrap()["result"],
            crate::overseer::discord::help::help_message()
        );
    }
}
