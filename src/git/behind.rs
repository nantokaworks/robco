//! Reading whether a pull request's branch has fallen behind its base, and
//! bringing it up to date on GitHub's own side.
//!
//! Split out of `remote.rs` to keep that file under this project's file-size
//! limit, not because the concern is unrelated: [`crate::git::merge_pr`]
//! reads [`pr_behind`] too, to name a merge refusal caused by the same
//! condition (dropr:574).

use std::{path::Path, process::Command};

use serde_json::Value;

use super::{
    GIT_NETWORK_TIMEOUT, command_output, command_unit, merge_lock::with_merge_lock_if_free,
};
use crate::{Error, Result, config::MergeStrategy, exec::run_timeout};

/// Reads `branch`'s pull request's own `mergeStateStatus`, or `None` when
/// GitHub did not report one — the same "treat an absent field as unknown,
/// never as a fact" rule `overseer::daemon::merge_state::merge_state` follows
/// for the same field, kept separate here because this read stands alone
/// (one field, one call) rather than riding along with a gate pass's fuller
/// `gh pr view`.
fn merge_state_status(repo: &Path, branch: &str) -> Result<Option<String>> {
    let mut command = Command::new("gh");
    command
        .current_dir(repo)
        .args(["pr", "view", branch, "--json", "mergeStateStatus"]);
    let output = run_timeout(command, GIT_NETWORK_TIMEOUT)?;
    let output = command_output(output, "gh pr view")?;
    Ok(parse_merge_state_status(&output))
}

fn parse_merge_state_status(json: &str) -> Option<String> {
    serde_json::from_str::<Value>(json)
        .ok()?
        .get("mergeStateStatus")
        .and_then(Value::as_str)
        .filter(|state| !state.is_empty())
        .map(str::to_string)
}

/// Whether GitHub currently reports `branch`'s pull request as behind its base.
pub fn pr_behind(repo: &Path, branch: &str) -> Result<bool> {
    Ok(merge_state_status(repo, branch)?.as_deref() == Some("BEHIND"))
}

/// What asking GitHub to update a pull request's branch actually did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchUpdateOutcome {
    /// GitHub reported the branch behind its base, and the update ran.
    Updated,
    /// GitHub did not report the branch behind its base, so nothing was sent.
    AlreadyUpToDate,
}

/// Brings `branch`'s pull request up to date with its base if GitHub reports
/// it behind, or reports that there is nothing to do. Shared by the TUI's `u`
/// key and the `robco_pr_update_branch` MCP tool (`crate::pr_update::update_behind`),
/// which is also what teaches the Overseer ledger about a successful update —
/// this function only ever talks to GitHub.
///
/// The behind check runs first so a branch that is merely current never pays
/// for `with_merge_lock_if_free`'s contention check, and so the caller's
/// three-way result (updated / already up to date / failed) never depends on
/// guessing what `gh pr update-branch` itself would have said about a no-op.
pub fn update_behind_branch(
    repo: &Path,
    branch: &str,
    strategy: MergeStrategy,
) -> Result<BranchUpdateOutcome> {
    if !pr_behind(repo, branch)? {
        return Ok(BranchUpdateOutcome::AlreadyUpToDate);
    }
    // Refused rather than queued, the same way `merge_flow::MergeFlow::run`
    // refuses: an operator update racing the daemon's own merge sequence for
    // this repository must not run underneath it, and there is no result
    // worth waiting for — the next press tries again.
    match with_merge_lock_if_free(repo, || {
        update_branch(repo, branch, update_branch_flag(strategy))
    })? {
        Some(()) => Ok(BranchUpdateOutcome::Updated),
        None => Err(Error::Command {
            context: "gh pr update-branch",
            stderr: format!(
                "a merge is currently running in {}; try again shortly",
                repo.display()
            ),
        }),
    }
}

/// Runs `gh pr update-branch <branch>`, optionally passing `flag` (e.g.
/// `--rebase` to keep a rebase-strategy repository's branch replayed rather
/// than merged onto — see [`update_branch_flag`]). Runs entirely on GitHub's
/// side, so it touches neither the local worktree nor the primary checkout.
///
/// Shared by the Overseer daemon's own auto-update pass
/// (`overseer::daemon::merge_state::run_update`) and the operator's actions
/// above, so both draw from one implementation (dropr:574).
pub fn update_branch(repo: &Path, branch: &str, flag: Option<&str>) -> Result<()> {
    let mut command = Command::new("gh");
    command
        .current_dir(repo)
        .args(["pr", "update-branch", branch]);
    command.args(flag);
    let output = run_timeout(command, GIT_NETWORK_TIMEOUT)?;
    command_unit(output, "gh pr update-branch")
}

/// Keeps a branch update consistent with the configured merge strategy: a
/// repository that merges by rebase gets a rebased branch rather than a merge
/// commit from the base, which would later make the rebase merge itself
/// impossible.
pub fn update_branch_flag(strategy: MergeStrategy) -> Option<&'static str> {
    (strategy == MergeStrategy::Rebase).then_some("--rebase")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_state_status_reads_the_field_and_treats_absence_as_unknown() {
        assert_eq!(
            parse_merge_state_status(r#"{"mergeStateStatus":"BEHIND"}"#),
            Some("BEHIND".to_string())
        );
        assert_eq!(parse_merge_state_status(r#"{"mergeStateStatus":""}"#), None);
        assert_eq!(parse_merge_state_status("{}"), None);
        assert_eq!(parse_merge_state_status("not json"), None);
    }

    #[test]
    fn only_a_rebase_strategy_rebases_the_branch_update() {
        assert_eq!(update_branch_flag(MergeStrategy::Rebase), Some("--rebase"));
        assert_eq!(update_branch_flag(MergeStrategy::Squash), None);
        assert_eq!(update_branch_flag(MergeStrategy::Merge), None);
    }
}
