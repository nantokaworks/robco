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

pub fn pr_exists(repo: &Path, branch: &str) -> Result<bool> {
    let mut command = Command::new("gh");
    command
        .current_dir(repo)
        .args(["pr", "list", "--head", branch, "--state", "open"])
        .args(["--json", "number"]);
    let output = run_timeout(command, GIT_NETWORK_TIMEOUT)?;
    let output = command_output(output, "gh pr list")?;
    Ok(!matches!(output.trim(), "" | "[]"))
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
