//! Catalog and dispatch coverage for the PR and merge tools. The handlers'
//! own logic is tested next to it in `pr.rs` and `merge.rs`; what is checked
//! here is that a client can actually reach them.

use serde_json::json;

use super::*;

/// The git-ops tools are declared in their own module. A name that never
/// reaches `list_tools` is a tool no client can call, however well `call_tool`
/// dispatches it.
#[test]
fn the_catalog_declares_the_pr_and_merge_tools() {
    let tools = catalog::list_tools();
    let tools = tools.as_array().unwrap();
    for name in ["robco_pr_status", "robco_pr_request", "robco_merge"] {
        assert!(
            tools.iter().any(|tool| tool["name"] == name),
            "{name} is missing from the catalog"
        );
    }
    let merge = find(tools, "robco_merge");
    assert_eq!(
        merge["inputSchema"]["required"],
        json!(["agent_id", "confirm"])
    );
    let status = find(tools, "robco_pr_status");
    assert_eq!(
        status["outputSchema"]["properties"]["pr_state"]["enum"],
        json!(["open", "merged", "closed_unmerged", "absent"])
    );
}

/// `robco_pr_request` sends text into a live session, so its description has to
/// say who writes the pull request — a controller that reads it as "robco opens
/// the PR" will report work that has not happened yet.
#[test]
fn the_pr_request_description_says_the_agent_authors_the_pull_request() {
    let tools = catalog::list_tools();
    let description = find(tools.as_array().unwrap(), "robco_pr_request")["description"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(description.contains("does not run `gh pr create`"));
}

#[test]
fn the_pr_and_merge_tools_validate_their_agent_id() {
    for name in ["robco_pr_status", "robco_pr_request", "robco_merge"] {
        let error = call_tool(name, Some(json!({ "agent_id": " " }))).unwrap_err();
        assert!(
            matches!(error, ToolError::InvalidParams(_)),
            "{name} accepted a blank agent_id"
        );
        let error = call_tool(name, Some(json!({}))).unwrap_err();
        assert!(
            matches!(error, ToolError::InvalidParams(_)),
            "{name} accepted a missing agent_id"
        );
    }
}

/// The gate between a controller agent and an irreversible merge. It refuses
/// before the registry is consulted, so a call that omitted it cannot reach
/// anything that touches the repository.
#[test]
fn a_merge_without_confirmation_is_refused() {
    let error = call_tool("robco_merge", Some(json!({ "agent_id": "a1" }))).unwrap_err();
    assert!(matches!(error, ToolError::InvalidParams(_)));
    assert!(error.to_string().contains("confirm must be true"));
}

fn find<'a>(tools: &'a [Value], name: &str) -> &'a Value {
    tools.iter().find(|tool| tool["name"] == name).unwrap()
}
