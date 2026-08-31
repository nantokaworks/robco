//! Bringing a pull request's branch up to date with its base, on the
//! operator's own request — shared by the TUI's `u` key
//! (`ui::actions::update_branch`) and the `robco_pr_update_branch` MCP tool.
//!
//! `git::update_behind_branch` runs entirely on GitHub's side and never
//! touches the worker's worktree or the primary checkout — see that
//! function's own doc. What this module adds on top is telling the Overseer
//! daemon about a successful update, so a ledger entry the auto-merge gate
//! parked on the automated update budget (`merge_state::UPDATE_CAP_REACHED`)
//! gets a fresh look rather than staying parked until an operator resorts to
//! answering the worker by hand (dropr:574).

use std::path::Path;

use crate::{
    Error, Result,
    config::MergeStrategy,
    git::{self, BranchUpdateOutcome, PrState},
    overseer::runtime_request::{self, RuntimeRequest},
};

/// What happened when an operator asked to update a pull request's branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOutcome {
    Updated,
    AlreadyUpToDate,
}

/// Updates `branch`'s pull request from its base if GitHub reports it behind,
/// and tells the daemon about a successful update so it resets the automated
/// budget that tracks the same thing.
///
/// Refuses when there is no open pull request at all: there is nothing on
/// GitHub for `gh pr update-branch` to act on, and reporting "already up to
/// date" for a branch with no pull request would tell the operator the wrong
/// thing.
pub fn update_behind(
    repo: &Path,
    branch: &str,
    agent_id: &str,
    strategy: MergeStrategy,
    source: &'static str,
) -> Result<UpdateOutcome> {
    if git::pr_state(repo, branch)? != PrState::Open {
        return Err(Error::NoOpenPullRequest(branch.to_string()));
    }
    let outcome = git::update_behind_branch(repo, branch, strategy)?;
    if outcome == BranchUpdateOutcome::Updated {
        // Best effort, the same way `merge_flow::MergeFlow::announce_merge`
        // treats waking the daemon: a failure to enqueue costs the delay this
        // removes, and there is nothing this caller could do about it anyway.
        let _ = runtime_request::enqueue(RuntimeRequest::BranchUpdated {
            source: source.to_string(),
            target: agent_id.to_string(),
            at: chrono::Utc::now(),
        });
    }
    Ok(match outcome {
        BranchUpdateOutcome::Updated => UpdateOutcome::Updated,
        BranchUpdateOutcome::AlreadyUpToDate => UpdateOutcome::AlreadyUpToDate,
    })
}
