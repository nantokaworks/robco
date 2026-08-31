//! `robco_pr_update_branch` — brings a pull request's branch up to date with
//! its base, the same action the TUI's `u` key runs (`crate::pr_update`).
//!
//! Non-destructive, unlike `robco_merge`: `gh pr update-branch` runs entirely
//! on GitHub's own side, so there is no worktree or branch to lose and no
//! `confirm` gate to pass.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{config::Config, pr_update, pr_update::UpdateOutcome, registry::Registry};

use super::{ToolResult, exec_err, find_agent, validate_non_blank};

const SOURCE: &str = "mcp";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PrUpdateBranchArgs {
    pub agent_id: String,
}

pub(super) fn pr_update_branch(args: PrUpdateBranchArgs) -> ToolResult<Value> {
    validate_non_blank("agent_id", &args.agent_id)?;
    let registry = Registry::load().map_err(exec_err)?;
    let (repo, agent) = find_agent(&registry, &args.agent_id)?;
    let strategy = Config::load().map_err(exec_err)?.merge_strategy;
    let outcome = pr_update::update_behind(&repo.path, &agent.branch, &agent.id, strategy, SOURCE)
        .map_err(exec_err)?;
    Ok(json!({
        "agent_id": agent.id,
        "branch": agent.branch,
        "outcome": outcome_label(outcome),
    }))
}

/// Stable wire names for [`UpdateOutcome`], spelled out rather than derived
/// from the variant names so renaming a variant cannot silently change the
/// tool's contract.
fn outcome_label(outcome: UpdateOutcome) -> &'static str {
    match outcome {
        UpdateOutcome::Updated => "updated",
        UpdateOutcome::AlreadyUpToDate => "already_up_to_date",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::{ToolError, parse_args};

    #[test]
    fn a_blank_agent_id_is_refused_before_touching_the_registry() {
        let args: PrUpdateBranchArgs = parse_args(Some(json!({ "agent_id": "  " }))).unwrap();
        let error = pr_update_branch(args).unwrap_err();
        assert!(matches!(error, ToolError::InvalidParams(_)));
    }

    #[test]
    fn each_outcome_has_a_distinct_wire_name() {
        assert_eq!(outcome_label(UpdateOutcome::Updated), "updated");
        assert_eq!(
            outcome_label(UpdateOutcome::AlreadyUpToDate),
            "already_up_to_date"
        );
    }
}
