use std::{
    path::Path,
    process::{Command, Output},
};

use super::{
    GIT_LOCAL_TIMEOUT, GIT_NETWORK_TIMEOUT, command_output, command_unit,
    merge_failure::{command_failure_text, explain_merge_failure},
};
use crate::{Error, Result, config::MergeStrategy, exec::run_timeout};

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

pub fn merge_pr(repo: &Path, branch: &str, strategy: MergeStrategy) -> Result<()> {
    let mut command = Command::new("gh");
    command
        .current_dir(repo)
        .args(["pr", "merge", branch, strategy.gh_flag()]);
    let output = run_timeout(command, GIT_NETWORK_TIMEOUT)?;
    if output.status.success() {
        return Ok(());
    }
    Err(Error::Command {
        context: "gh pr merge",
        stderr: refusal_detail(strategy, &output),
    })
}

/// The failure an operator reads. A refusal robco can explain leads with the
/// cause, because `gh`'s own line names the exit rather than the branch shape
/// behind it; the raw output still follows, so nothing is hidden.
fn refusal_detail(strategy: MergeStrategy, output: &Output) -> String {
    let raw = command_failure_text(output);
    match explain_merge_failure(strategy, &raw) {
        Some(refusal) => format!(
            "{} refused: {} (gh: {raw})",
            strategy.label(),
            refusal.message
        ),
        None => raw,
    }
}

/// Updates the repository's knowledge of `origin/<branch>` without touching
/// whatever the repository has checked out.
///
/// Unlike `git pull`, `git fetch` never reads or writes the working tree, the
/// index, or `HEAD` — only `refs/remotes/origin/*` and `FETCH_HEAD` — so it
/// stays safe to run against a checkout an operator or another robco process
/// may be sitting in, dirty or on any branch, at any time.
pub fn fetch_branch(repo: &Path, branch: &str) -> Result<()> {
    let mut command = Command::new("git");
    command
        .args(["-C"])
        .arg(repo)
        .args(["fetch", "origin", branch]);
    let output = run_timeout(command, GIT_NETWORK_TIMEOUT)?;
    command_unit(output, "git fetch origin")
}

/// The commit `origin/<branch>` points to, after fetching it fresh. Never
/// touches the working tree — see [`fetch_branch`].
pub fn remote_branch_commit(repo: &Path, branch: &str) -> Result<String> {
    fetch_branch(repo, branch)?;
    let mut command = Command::new("git");
    command
        .args(["-C"])
        .arg(repo)
        .args(["rev-parse"])
        .arg(format!("origin/{branch}"));
    let output = run_timeout(command, GIT_LOCAL_TIMEOUT)?;
    command_output(output, "git rev-parse origin branch")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::test_repo::TestRepo;

    /// The point of routing dispatch and cleanup through this function rather
    /// than a plain `git pull`: it learns the remote branch's commit without
    /// ever moving whatever the repository has checked out.
    #[test]
    fn remote_branch_commit_fetches_without_touching_the_checkout() {
        let repo = TestRepo::new();
        let branch_before = branch_name(repo.path());

        let commit = remote_branch_commit(repo.path(), "main").unwrap();

        assert_eq!(commit, rev_parse(repo.path(), "origin/main"));
        assert_eq!(branch_name(repo.path()), branch_before);
    }

    fn branch_name(repo: &Path) -> String {
        let output = Command::new("git")
            .args(["-C"])
            .arg(repo)
            .args(["symbolic-ref", "--short", "HEAD"])
            .output()
            .unwrap();
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    fn rev_parse(repo: &Path, reference: &str) -> String {
        let output = Command::new("git")
            .args(["-C"])
            .arg(repo)
            .args(["rev-parse", reference])
            .output()
            .unwrap();
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

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
