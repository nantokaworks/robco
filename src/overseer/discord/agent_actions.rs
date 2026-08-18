//! The six commands Discord gained from MCP: `Whoami`, `Report`,
//! `AgentCreate`, `QuestionList`, `PrStatus`, `PrRequest`.
//!
//! Each one's business logic already lives in `crate::mcp::tools` — tested,
//! and in `AgentCreate`'s case backed by a declared MCP `outputSchema` other
//! clients depend on — so these wrappers call straight into it and format
//! the JSON result as chat text, instead of holding a second implementation.
//! See `dropr:463`'s decision scribble for why the sharing runs this
//! direction for these six instead of through `actions::execute`.

use serde_json::Value;

use crate::mcp::tools::{self, ToolError};

pub(super) fn whoami() -> crate::Result<String> {
    to_reply(tools::whoami())
}

pub(super) fn report(message: &str, target_agent_id: Option<&str>) -> crate::Result<String> {
    tools::deliver_report(message, target_agent_id).map_err(tool_err)?;
    Ok("report delivered".into())
}

pub(super) fn agent_create(
    repo: &str,
    title: &str,
    prompt: Option<&str>,
    parent_agent_id: Option<&str>,
    autonomous: bool,
) -> crate::Result<String> {
    to_reply(tools::agent_create(
        repo,
        title,
        prompt,
        parent_agent_id,
        autonomous,
    ))
}

pub(super) fn question_list() -> crate::Result<String> {
    to_reply(tools::question_list())
}

pub(super) fn pr_status(agent: &str) -> crate::Result<String> {
    to_reply(tools::pr_status(agent))
}

pub(super) fn pr_request(agent: &str, prompt: Option<&str>) -> crate::Result<String> {
    to_reply(tools::pr_request(agent, prompt))
}

fn to_reply(result: Result<Value, ToolError>) -> crate::Result<String> {
    let value = result.map_err(tool_err)?;
    serde_json::to_string_pretty(&value)
        .map(|json| format!("```json\n{json}\n```"))
        .map_err(Into::into)
}

fn tool_err(error: ToolError) -> crate::Error {
    std::io::Error::other(error.to_string()).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn to_reply_renders_the_value_as_a_fenced_json_block() {
        let rendered = to_reply(Ok(json!({ "ok": true }))).unwrap();
        assert!(rendered.starts_with("```json\n"));
        assert!(rendered.ends_with("\n```"));
        assert!(rendered.contains("\"ok\": true"));
    }

    #[test]
    fn to_reply_surfaces_a_tool_error_as_a_crate_error() {
        let error = to_reply(Err(ToolError::InvalidParams("bad input".into()))).unwrap_err();
        assert!(error.to_string().contains("bad input"));
    }

    #[test]
    fn whoami_with_no_identity_still_replies() {
        // No ROBCO_AGENT_ID in this test process, so this exercises the
        // "identity absent" branch of `identity::whoami_with_lookup` through
        // the whole Discord -> MCP path, not just a mocked lookup.
        assert!(whoami().is_ok());
    }
}
