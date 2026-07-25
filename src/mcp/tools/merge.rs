//! `robco_merge` — the destructive half of the MCP surface.
//!
//! It runs the same sequence the TUI's merge key runs (see
//! [`crate::git::merge_flow`]), so a worker landed over MCP ends up in the same
//! state as one landed by hand. Because nothing about it can be undone by
//! robco, it is gated twice: the caller must pass `confirm: true`, and the
//! branch's pull request must already be in the state the requested mode
//! expects. Neither gate protects against a caller that means it — that is what
//! they are for.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    agent,
    config::Config,
    git::{
        PrState,
        merge_flow::{MergeFlow, MergeMode},
    },
    registry::Registry,
};

use super::{
    ToolResult, exec_err, find_agent, invalid_params, pr::state_label, validate_non_blank,
};

/// Names the caller on the `MergeCompleted` runtime request, so the daemon log
/// distinguishes an MCP merge from one the operator pressed.
const SOURCE: &str = "mcp";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MergeArgs {
    pub agent_id: String,
    #[serde(default)]
    pub mode: RequestedMode,
    /// Deliberately not defaulted to true. Merging deletes the branch and the
    /// worktree; a caller has to say so on purpose.
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RequestedMode {
    #[default]
    MergeThenClean,
    CleanOnly,
}

impl RequestedMode {
    fn label(self) -> &'static str {
        match self {
            Self::MergeThenClean => "merge_then_clean",
            Self::CleanOnly => "clean_only",
        }
    }

    fn flow_mode(self) -> MergeMode {
        match self {
            Self::MergeThenClean => MergeMode::MergeThenClean,
            Self::CleanOnly => MergeMode::CleanOnly,
        }
    }

    /// The pull request state the mode is the right answer to. Running the
    /// wrong one is how a branch gets deleted with work still only on it, so
    /// the mismatch is refused rather than reported afterwards.
    fn requires(self) -> PrState {
        match self {
            Self::MergeThenClean => PrState::Open,
            Self::CleanOnly => PrState::Merged,
        }
    }
}

/// The confirmation gate runs before the registry is even read, so a call that
/// forgot it cannot reach anything that touches the repository.
pub(super) fn merge(args: MergeArgs) -> ToolResult<Value> {
    validate_non_blank("agent_id", &args.agent_id)?;
    if !args.confirm {
        return Err(invalid_params(
            "confirm must be true: robco_merge merges the pull request, removes the worktree, \
             and deletes the branch, and robco cannot undo any of it",
        ));
    }
    let registry = Registry::load().map_err(exec_err)?;
    merge_confirmed(&registry, &args)
}

fn merge_confirmed(registry: &Registry, args: &MergeArgs) -> ToolResult<Value> {
    let (repo, agent) = find_agent(registry, &args.agent_id)?;
    let state = crate::git::pr_state(&repo.path, &agent.branch).map_err(exec_err)?;
    if state != args.mode.requires() {
        return Err(exec_err(format!(
            "{} needs a {} pull request for {}, found {}",
            args.mode.label(),
            state_label(args.mode.requires()),
            agent.branch,
            state_label(state)
        )));
    }
    let strategy = Config::load().map_err(exec_err)?.merge_strategy;
    let mut steps = Vec::new();
    let result = MergeFlow {
        repo: &repo.path,
        branch: &agent.branch,
        worktree: &agent.worktree_path,
        tmux_session: &agent.tmux_session,
        shell_session: &agent::shell_session_name(agent),
        mode: args.mode.flow_mode(),
        strategy,
        source: SOURCE,
    }
    .run(|step| steps.push(step));

    match result {
        Ok(()) => {
            forget_agent(&args.agent_id);
            Ok(outcome(args, &agent.branch, &steps, None))
        }
        // A sequence that fails is still a tool call that ran. The failure is
        // reported in the payload, with the step it stopped on, so a controller
        // can act on it without reading the error text.
        Err(error) => Ok(outcome(
            args,
            &agent.branch,
            &steps,
            Some(error.to_string()),
        )),
    }
}

fn outcome(args: &MergeArgs, branch: &str, steps: &[&str], error: Option<String>) -> Value {
    json!({
        "ok": error.is_none(),
        "agent_id": args.agent_id,
        "branch": branch,
        "mode": args.mode.label(),
        "steps": steps,
        "failed_step": error.as_ref().and(steps.last().copied()),
        "error": error,
    })
}

/// Drop the merged worker's registry row, which is what the TUI does once its
/// merge finishes. Without it the agent would linger in `robco_agent_list` with
/// no worktree behind it.
///
/// Best effort, and deliberately not allowed to fail the call: by this point the
/// pull request is merged and the branch is gone, and reporting that as a failed
/// merge would be a worse lie than a stale row. The Overseer's next reconcile
/// pass drops rows for merged workers anyway.
fn forget_agent(agent_id: &str) {
    let _ = Registry::locked_update(|registry| {
        for repo in &mut registry.repos {
            repo.agents.retain(|agent| agent.id != agent_id);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::{ToolError, parse_args, tests::registry_with_agent};

    #[test]
    fn mode_defaults_to_merging_and_accepts_the_clean_only_spelling() {
        let args: MergeArgs =
            parse_args(Some(json!({ "agent_id": "a1", "confirm": true }))).unwrap();
        assert_eq!(args.mode, RequestedMode::MergeThenClean);
        let args: MergeArgs = parse_args(Some(
            json!({ "agent_id": "a1", "mode": "clean_only", "confirm": true }),
        ))
        .unwrap();
        assert_eq!(args.mode, RequestedMode::CleanOnly);
        assert!(
            parse_args::<MergeArgs>(Some(json!({ "agent_id": "a1", "mode": "nope" }))).is_err()
        );
    }

    /// Each mode is only correct for one pull request state, and the tool has to
    /// know which before it runs anything.
    #[test]
    fn each_mode_requires_the_pull_request_state_it_is_the_answer_to() {
        assert_eq!(RequestedMode::MergeThenClean.requires(), PrState::Open);
        assert_eq!(RequestedMode::CleanOnly.requires(), PrState::Merged);
    }

    #[test]
    fn an_unconfirmed_merge_is_refused_before_anything_runs() {
        let error = merge(MergeArgs {
            agent_id: "a1".into(),
            mode: RequestedMode::MergeThenClean,
            confirm: false,
        })
        .unwrap_err();

        assert!(matches!(error, ToolError::InvalidParams(_)));
        assert!(error.to_string().contains("confirm must be true"));
    }

    #[test]
    fn an_unknown_agent_is_refused_even_when_confirmed() {
        let registry = registry_with_agent("a1");
        let error = merge_confirmed(
            &registry,
            &MergeArgs {
                agent_id: "missing".into(),
                mode: RequestedMode::MergeThenClean,
                confirm: true,
            },
        )
        .unwrap_err();

        assert!(matches!(error, ToolError::Execution(_)));
        assert!(error.to_string().contains("agent_id not found"));
    }

    #[test]
    fn a_failed_sequence_reports_the_step_it_stopped_on() {
        let payload = outcome(
            &MergeArgs {
                agent_id: "a1".into(),
                mode: RequestedMode::CleanOnly,
                confirm: true,
            },
            "feature",
            &["pulling main"],
            Some("git pull --ff-only failed".into()),
        );

        assert_eq!(payload["ok"], false);
        assert_eq!(payload["mode"], "clean_only");
        assert_eq!(payload["failed_step"], "pulling main");
        assert_eq!(payload["error"], "git pull --ff-only failed");
    }

    #[test]
    fn a_successful_sequence_reports_no_failed_step() {
        let payload = outcome(
            &MergeArgs {
                agent_id: "a1".into(),
                mode: RequestedMode::MergeThenClean,
                confirm: true,
            },
            "feature",
            &["merging PR", "pulling main", "cleaning up"],
            None,
        );

        assert_eq!(payload["ok"], true);
        assert_eq!(payload["steps"][0], "merging PR");
        assert!(payload["failed_step"].is_null());
        assert!(payload["error"].is_null());
    }
}
