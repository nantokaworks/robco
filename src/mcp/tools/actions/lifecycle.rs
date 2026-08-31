//! Worker lifecycle actions. Killing is destructive, so its explicit
//! confirmation gate signals intent; it does not authenticate the caller.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{agent, git, model::Status, registry::Registry};

use super::super::{
    ToolResult, exec_err, find_agent, invalid_params, live_status, validate_non_blank,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct KillArgs {
    agent_id: String,
    #[serde(default)]
    force: bool,
    #[serde(default)]
    delete_branch: bool,
    #[serde(default)]
    confirm: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RestartArgs {
    agent_id: String,
}

pub(super) fn kill(args: KillArgs) -> ToolResult<Value> {
    validate_non_blank("agent_id", &args.agent_id)?;
    if !args.confirm {
        return Err(invalid_params(
            "confirm must be true: robco_agent_kill removes a worker and may delete its branch",
        ));
    }
    let registry = Registry::load().map_err(exec_err)?;
    let (repo, agent) = find_agent(&registry, &args.agent_id)?;
    let repo_path = repo.path.clone();
    let branch = agent.branch.clone();
    let status = live_status(repo, agent).status;
    let outcome = git::merge_lock::with_merge_lock_if_free(&repo_path, || {
        if status != Status::BranchOnly {
            agent::kill_agent(repo, agent, args.force)?;
        }
        let existed = git::branch_exists(&repo_path, &branch)?;
        if existed && args.delete_branch {
            git::delete_branch(&repo_path, &branch)?;
        }
        Ok((
            existed && !args.delete_branch,
            existed && args.delete_branch,
        ))
    })
    .map_err(exec_err)?
    .ok_or_else(|| exec_err("cannot kill an agent while it is merging"))?;
    if !outcome.0 {
        forget_agent(&args.agent_id).map_err(exec_err)?;
    }
    Ok(json!({
        "ok": true,
        "agent_id": args.agent_id,
        "force": args.force,
        "branch": branch,
        "branch_remains": outcome.0,
        "branch_deleted": outcome.1,
    }))
}

pub(super) fn restart(args: RestartArgs) -> ToolResult<Value> {
    validate_non_blank("agent_id", &args.agent_id)?;
    let registry = Registry::load().map_err(exec_err)?;
    let (repo, selected) = find_agent(&registry, &args.agent_id)?;
    if live_status(repo, selected).status == Status::BranchOnly {
        return Err(exec_err(format!("branch remains: {}", selected.branch)));
    }
    let restarted =
        git::merge_lock::with_merge_lock_if_free(&repo.path, || agent::restart_agent(selected))
            .map_err(exec_err)?
            .is_some();
    if !restarted {
        return Err(exec_err("cannot restart an agent while it is merging"));
    }
    Ok(json!({ "ok": true, "agent_id": args.agent_id }))
}

fn forget_agent(agent_id: &str) -> crate::Result<()> {
    Registry::locked_update(|registry| {
        for repo in &mut registry.repos {
            repo.agents.retain(|agent| agent.id != agent_id);
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::{ToolError, parse_args};

    #[test]
    fn kill_requires_an_explicit_confirmation() {
        let error = kill(parse_args(Some(json!({ "agent_id": "a1" }))).unwrap()).unwrap_err();
        assert!(matches!(error, ToolError::InvalidParams(_)));
        assert!(error.to_string().contains("confirm must be true"));
    }

    #[test]
    fn lifecycle_args_reject_unknown_fields() {
        assert!(parse_args::<RestartArgs>(Some(json!({"agent_id":"a1","force":true}))).is_err());
    }
}
