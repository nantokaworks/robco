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

/// The repository's own default branch — the branch its clone came with.
///
/// Reads `refs/remotes/origin/HEAD`, a local pointer `git clone` sets by
/// itself (or an operator repairs by hand with `git remote set-head origin
/// -a`). Never touches the network and never guesses: a repository whose
/// remote `HEAD` was never fetched, or one created locally with no remote,
/// answers `None` rather than a hardcoded branch name — see dropr:503.
pub fn default_branch(repo: &Path) -> Result<Option<String>> {
    let mut command = Command::new("git");
    command.args(["-C"]).arg(repo).args([
        "symbolic-ref",
        "--short",
        "-q",
        "refs/remotes/origin/HEAD",
    ]);
    let output = run_timeout(command, GIT_LOCAL_TIMEOUT)?;
    if !output.status.success() {
        return Ok(None);
    }
    let full = command_output(output, "git symbolic-ref refs/remotes/origin/HEAD")?;
    Ok(Some(
        full.strip_prefix("origin/").unwrap_or(&full).to_string(),
    ))
}

/// The branch `HEAD` currently points at, or `None` when it is detached.
pub fn current_branch(repo: &Path) -> Result<Option<String>> {
    let mut command = Command::new("git");
    command
        .args(["-C"])
        .arg(repo)
        .args(["symbolic-ref", "--short", "-q", "HEAD"]);
    let output = run_timeout(command, GIT_LOCAL_TIMEOUT)?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(command_output(output, "git symbolic-ref")?))
}

/// Fast-forwards whatever is checked out in `repo` onto `target`, moving the
/// working tree and the index along with the branch ref. Refuses on its own —
/// leaving the checkout untouched — when `target` is not a descendant of the
/// current `HEAD`, which is why this is safe to call without a divergence
/// check of its own.
pub fn fast_forward_checkout(repo: &Path, target: &str) -> Result<()> {
    let mut command = Command::new("git");
    command
        .args(["-C"])
        .arg(repo)
        .args(["merge", "--ff-only", target]);
    let output = run_timeout(command, GIT_LOCAL_TIMEOUT)?;
    command_unit(output, "git merge --ff-only")
}

/// Advances the local `branch` ref straight to `from` — typically
/// `origin/<branch>`, already fetched — without touching whatever the
/// repository has checked out. Reads purely from local refs (`repository`
/// argument `.`), so this never hits the network even though it goes through
/// `git fetch`. Refuses on its own when `branch` is currently checked out
/// elsewhere or `from` is not a fast-forward, exactly like a real fetch would
/// refuse a non-fast-forward ref update.
pub fn fast_forward_ref(repo: &Path, branch: &str, from: &str) -> Result<()> {
    let mut command = Command::new("git");
    command
        .args(["-C"])
        .arg(repo)
        .args(["fetch", "-q", "."])
        .arg(format!("{from}:{branch}"));
    let output = run_timeout(command, GIT_LOCAL_TIMEOUT)?;
    command_unit(output, "git fetch . (local ref fast-forward)")
}

#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;
