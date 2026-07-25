use std::{path::Path, process::Command};

use super::{GIT_NETWORK_TIMEOUT, command_output, command_unit};
use crate::{Result, exec::run_timeout};

pub fn delete_remote_branch(repo: &Path, branch: &str) -> Result<()> {
    let mut command = Command::new("git");
    command
        .args(["-C"])
        .arg(repo)
        .args(["push", "origin", "--delete", branch]);
    let output = run_timeout(command, GIT_NETWORK_TIMEOUT)?;
    command_unit(output, "git push origin --delete")
}

/// What the branch's pull requests amount to, for a caller deciding between
/// merging, cleaning up after a merge someone else did, and doing neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrState {
    /// At least one pull request is open, so it can still be merged.
    Open,
    /// None are open and one landed. The merge already happened.
    Merged,
    /// Every pull request was closed without merging, so the branch still
    /// holds work that is nowhere else.
    ClosedUnmerged,
    /// The branch has never had a pull request.
    Absent,
}

#[derive(serde::Deserialize)]
struct PrListEntry {
    state: String,
}

pub fn pr_exists(repo: &Path, branch: &str) -> Result<bool> {
    Ok(pr_state(repo, branch)? == PrState::Open)
}

/// Reads every pull request opened from `branch`, whatever its state.
///
/// `gh pr list` is asked for `--state all` rather than `open` because the
/// caller has to tell a merged pull request from one closed unmerged and from
/// a branch that never had one — three cases an open-only query flattens into
/// "no PR".
pub fn pr_state(repo: &Path, branch: &str) -> Result<PrState> {
    let mut command = Command::new("gh");
    command
        .current_dir(repo)
        .args(["pr", "list", "--head", branch, "--state", "all"])
        .args(["--json", "state"]);
    let output = run_timeout(command, GIT_NETWORK_TIMEOUT)?;
    let output = command_output(output, "gh pr list")?;
    pr_state_from_list(&output)
}

/// A branch can carry several pull requests at once — a closed attempt, a
/// merged one, a reopened one — so the states are ranked rather than read off
/// the first entry: an open pull request is still mergeable, and a merge that
/// landed outweighs an attempt that was abandoned.
fn pr_state_from_list(json: &str) -> Result<PrState> {
    let json = json.trim();
    if json.is_empty() {
        return Ok(PrState::Absent);
    }
    let entries: Vec<PrListEntry> = serde_json::from_str(json)?;
    if entries.is_empty() {
        return Ok(PrState::Absent);
    }
    let state = |wanted: &str| entries.iter().any(|entry| entry.state == wanted);
    Ok(if state("OPEN") {
        PrState::Open
    } else if state("MERGED") {
        PrState::Merged
    } else {
        PrState::ClosedUnmerged
    })
}

pub fn merge_pr(repo: &Path, branch: &str, strategy_flag: &str) -> Result<()> {
    let mut command = Command::new("gh");
    command
        .current_dir(repo)
        .args(["pr", "merge", branch, strategy_flag]);
    let output = run_timeout(command, GIT_NETWORK_TIMEOUT)?;
    command_unit(output, "gh pr merge")
}

pub fn pull_ff_only(main_worktree: &Path) -> Result<()> {
    let mut command = Command::new("git");
    command
        .args(["-C"])
        .arg(main_worktree)
        .args(["pull", "--ff-only"]);
    let output = run_timeout(command, GIT_NETWORK_TIMEOUT)?;
    command_unit(output, "git pull --ff-only")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_branch_without_pull_requests_is_absent() {
        assert_eq!(pr_state_from_list("[]").unwrap(), PrState::Absent);
        assert_eq!(pr_state_from_list("").unwrap(), PrState::Absent);
    }

    #[test]
    fn each_terminal_state_is_distinguished() {
        assert_eq!(
            pr_state_from_list(r#"[{"state":"MERGED"}]"#).unwrap(),
            PrState::Merged
        );
        assert_eq!(
            pr_state_from_list(r#"[{"state":"CLOSED"}]"#).unwrap(),
            PrState::ClosedUnmerged
        );
        assert_eq!(
            pr_state_from_list(r#"[{"state":"OPEN"}]"#).unwrap(),
            PrState::Open
        );
    }

    #[test]
    fn an_open_pull_request_outranks_earlier_attempts() {
        assert_eq!(
            pr_state_from_list(r#"[{"state":"CLOSED"},{"state":"OPEN"},{"state":"MERGED"}]"#)
                .unwrap(),
            PrState::Open
        );
    }

    #[test]
    fn a_merge_outranks_an_abandoned_attempt() {
        assert_eq!(
            pr_state_from_list(r#"[{"state":"CLOSED"},{"state":"MERGED"}]"#).unwrap(),
            PrState::Merged
        );
    }

    /// Unreadable output must not read as "no pull request": that is the one
    /// answer that tells the user to open one they may already have.
    #[test]
    fn unreadable_output_is_an_error() {
        assert!(pr_state_from_list("not json").is_err());
    }
}
