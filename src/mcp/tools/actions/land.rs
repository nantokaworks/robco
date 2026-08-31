//! One-call landing decision tree. Because this can merge and delete a
//! worktree, its confirmation gate signals intent; it does not authenticate.

use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    git::{PrChecks, PrState},
    overseer::runtime_request::{self, RuntimeRequest},
    registry::Registry,
};

use super::super::{
    ToolResult, exec_err, find_agent, invalid_params,
    merge::{self, MergeArgs, RequestedMode},
    pr::{self, PrRequestArgs},
    validate_non_blank,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LandArgs {
    agent_id: String,
    #[serde(default)]
    confirm: bool,
}

pub(super) fn land(args: LandArgs) -> ToolResult<Value> {
    validate_non_blank("agent_id", &args.agent_id)?;
    if !args.confirm {
        return Err(invalid_params(
            "confirm must be true: robco_agent_land may merge and remove the worker",
        ));
    }
    let registry = Registry::load().map_err(exec_err)?;
    let (repo, agent) = find_agent(&registry, &args.agent_id)?;
    let state = crate::git::pr_state(&repo.path, &agent.branch).map_err(exec_err)?;
    match state {
        PrState::Absent => {
            let head =
                crate::git::local_branch_commit(&repo.path, &agent.branch).map_err(exec_err)?;
            let request = pr::pr_request(PrRequestArgs {
                agent_id: args.agent_id.clone(),
                prompt: None,
            })?;
            queue(&args.agent_id, head.clone())?;
            Ok(json!({
                "ok": true,
                "action": "open_pr_then_queue",
                "agent_id": args.agent_id,
                "head": head,
                "pr_request": request,
            }))
        }
        PrState::Merged => run_merge(args.agent_id, RequestedMode::CleanOnly, "cleanup"),
        PrState::ClosedUnmerged => Err(exec_err(
            "pull request was closed without merging; reopen it or open a new one",
        )),
        PrState::Open => land_open(repo, agent, args.agent_id),
    }
}

fn land_open(
    repo: &crate::model::RepoNode,
    agent: &crate::model::AgentNode,
    agent_id: String,
) -> ToolResult<Value> {
    let view = crate::git::pr_checks(&repo.path, &agent.branch).map_err(exec_err)?;
    match view.checks {
        PrChecks::Green => run_merge(agent_id, RequestedMode::MergeThenClean, "merge_now"),
        PrChecks::Waiting => {
            queue(&agent_id, view.head.clone())?;
            Ok(json!({
                "ok": true,
                "action": "queue_approval",
                "agent_id": agent_id,
                "head": view.head,
            }))
        }
        PrChecks::Failed(names) => Ok(json!({
            "ok": false,
            "action": "refused_failed_checks",
            "agent_id": agent_id,
            "failed_checks": names,
        })),
    }
}

fn run_merge(agent_id: String, mode: RequestedMode, action: &str) -> ToolResult<Value> {
    let result = merge::merge(MergeArgs {
        agent_id: agent_id.clone(),
        mode,
        confirm: true,
    })?;
    Ok(json!({
        "ok": result.get("ok").and_then(Value::as_bool).unwrap_or(false),
        "action": action,
        "agent_id": agent_id,
        "merge": result,
    }))
}

fn queue(target: &str, head: String) -> ToolResult<()> {
    runtime_request::enqueue(RuntimeRequest::MergeApproval {
        source: "mcp".into(),
        target: target.into(),
        head,
        at: Utc::now(),
    })
    .map_err(exec_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::{ToolError, parse_args};

    #[test]
    fn landing_requires_confirmation() {
        let error = land(parse_args(Some(json!({"agent_id":"a1"}))).unwrap()).unwrap_err();
        assert!(matches!(error, ToolError::InvalidParams(_)));
    }

    #[test]
    fn land_arguments_reject_unknown_fields() {
        assert!(parse_args::<LandArgs>(Some(json!({"agent_id":"a1","yes":true}))).is_err());
    }
}
