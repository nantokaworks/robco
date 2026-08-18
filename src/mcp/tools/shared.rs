//! Crate-visible wrappers around the MCP tool implementations that
//! `Command::Whoami` / `Report` / `AgentCreate` / `QuestionList` / `PrStatus`
//! / `PrRequest` share with Discord (`discord::agent_actions`), so Discord
//! never has to reach past this module's boundary into the private `*Args`
//! structs each tool actually takes.

use serde_json::Value;

use super::{ToolResult, pr, questions, spawn};
use crate::mcp::tools::identity;

pub(crate) fn whoami() -> ToolResult<Value> {
    identity::whoami()
}

pub(crate) fn agent_create(
    repo: &str,
    title: &str,
    prompt: Option<&str>,
    parent_agent_id: Option<&str>,
    autonomous: bool,
) -> ToolResult<Value> {
    spawn::spawn(spawn::SpawnArgs {
        repo: repo.to_string(),
        title: title.to_string(),
        prompt: prompt.map(String::from),
        parent_agent_id: parent_agent_id.map(String::from),
        autonomous,
    })
}

pub(crate) fn question_list() -> ToolResult<Value> {
    questions::list()
}

pub(crate) fn pr_status(agent_id: &str) -> ToolResult<Value> {
    pr::pr_status(pr::PrStatusArgs {
        agent_id: agent_id.to_string(),
    })
}

pub(crate) fn pr_request(agent_id: &str, prompt: Option<&str>) -> ToolResult<Value> {
    pr::pr_request(pr::PrRequestArgs {
        agent_id: agent_id.to_string(),
        prompt: prompt.map(String::from),
    })
}
