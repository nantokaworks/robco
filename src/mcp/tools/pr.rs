//! The read and request halves of the PR flow, as MCP tools.
//!
//! `robco_pr_status` reports what a worker's branch has on GitHub; the caller
//! needs it to tell "no PR yet" from "open" from "already merged", which is the
//! same three-way split the TUI's merge key branches on.
//!
//! `robco_pr_request` is deliberately not `gh pr create`. Authoring the pull
//! request is the worker's job — robco only delivers the prompt that asks for
//! it, exactly as the TUI's confirm dialog does.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{config::Config, git::PrState, registry::Registry, tmux};

use super::{ToolError, ToolResult, exec_err, find_agent, validate_non_blank};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PrStatusArgs {
    pub agent_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PrRequestArgs {
    pub agent_id: String,
    /// Omitted means "use the prompt the TUI pre-fills its dialog with", so a
    /// controller that has no opinion sends what the operator configured.
    #[serde(default)]
    pub prompt: Option<String>,
}

pub(super) fn pr_status(args: PrStatusArgs) -> ToolResult<Value> {
    validate_non_blank("agent_id", &args.agent_id)?;
    let registry = Registry::load().map_err(exec_err)?;
    status_in(&registry, &args.agent_id)
}

fn status_in(registry: &Registry, agent_id: &str) -> ToolResult<Value> {
    let (repo, agent) = find_agent(registry, agent_id)?;
    let state = crate::git::pr_state(&repo.path, &agent.branch).map_err(exec_err)?;
    Ok(json!({
        "agent_id": agent.id,
        "branch": agent.branch,
        "pr_state": state_label(state),
    }))
}

/// Stable wire names for [`PrState`]. Spelled out rather than derived from the
/// variant names so renaming a variant cannot silently change the tool's
/// contract.
pub(super) const fn state_label(state: PrState) -> &'static str {
    match state {
        PrState::Open => "open",
        PrState::Merged => "merged",
        PrState::ClosedUnmerged => "closed_unmerged",
        PrState::Absent => "absent",
    }
}

pub(super) fn pr_request(args: PrRequestArgs) -> ToolResult<Value> {
    validate_non_blank("agent_id", &args.agent_id)?;
    let registry = Registry::load().map_err(exec_err)?;
    request_in(&registry, &args.agent_id, args.prompt.as_deref())
}

fn request_in(registry: &Registry, agent_id: &str, prompt: Option<&str>) -> ToolResult<Value> {
    let (repo, agent) = find_agent(registry, agent_id)?;
    let prompt = resolve_prompt(prompt)?;
    // The refusal is an execution error rather than invalid params: the
    // arguments were fine, the worker's state is what says no.
    crate::pr::precheck(&repo.path, &agent.branch, &agent.tmux_session)
        .map_err(ToolError::Execution)?;
    tmux::send_literal_text(&agent.tmux_session, &prompt).map_err(exec_err)?;
    tmux::send_keys(&agent.tmux_session, &["Enter"]).map_err(exec_err)?;
    Ok(json!({
        "ok": true,
        "agent_id": agent.id,
        "branch": agent.branch,
    }))
}

fn resolve_prompt(prompt: Option<&str>) -> ToolResult<String> {
    match prompt {
        Some(prompt) => {
            validate_non_blank("prompt", prompt)?;
            Ok(prompt.to_string())
        }
        None => Ok(Config::load().map_err(exec_err)?.pr_prompt),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::tests::registry_with_agent;

    /// Both tools resolve the agent before they reach `gh` or tmux, so an id
    /// that is not in the registry costs nothing and says so plainly.
    #[test]
    fn an_unknown_agent_is_refused_by_both_tools() {
        let registry = registry_with_agent("a1");
        for error in [
            status_in(&registry, "missing").unwrap_err(),
            request_in(&registry, "missing", Some("open the PR")).unwrap_err(),
        ] {
            assert!(matches!(error, ToolError::Execution(_)));
            assert!(error.to_string().contains("agent_id not found: missing"));
        }
    }

    #[test]
    fn each_pr_state_has_a_distinct_wire_name() {
        let labels = [
            state_label(PrState::Open),
            state_label(PrState::Merged),
            state_label(PrState::ClosedUnmerged),
            state_label(PrState::Absent),
        ];
        assert_eq!(labels, ["open", "merged", "closed_unmerged", "absent"]);
    }

    #[test]
    fn an_explicit_prompt_is_used_verbatim_and_a_blank_one_refused() {
        assert_eq!(resolve_prompt(Some("open the PR")).unwrap(), "open the PR");
        assert!(matches!(
            resolve_prompt(Some("  ")),
            Err(ToolError::InvalidParams(_))
        ));
    }
}
