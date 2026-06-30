use std::{path::Path, process::Command};

use crate::{Error, Result};

pub fn remote_url(repo: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["remote", "get-url", "origin"])
        .output()?;
    command_output(output, "git remote get-url origin")
}

pub fn head_commit(repo: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["rev-parse", "HEAD"])
        .output()?;
    command_output(output, "git rev-parse HEAD")
}

pub fn worktree_add_command(repo: &Path, worktree: &Path, branch: &str, base: &str) -> Command {
    let mut command = Command::new("git");
    command
        .args(["-C"])
        .arg(repo)
        .args(["worktree", "add"])
        .arg(worktree)
        .args(["-b", branch, base]);
    command
}

pub fn add_worktree(repo: &Path, worktree: &Path, branch: &str, base: &str) -> Result<()> {
    let output = worktree_add_command(repo, worktree, branch, base).output()?;
    command_unit(output, "git worktree add")
}

pub fn remove_worktree(repo: &Path, worktree: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["worktree", "remove"])
        .arg(worktree)
        .output()?;
    command_unit(output, "git worktree remove")?;

    let output = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["worktree", "prune"])
        .output()?;
    command_unit(output, "git worktree prune")
}

pub fn tracked_tree_is_clean(worktree: &Path) -> Result<bool> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(worktree)
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()?;
    Ok(command_output(output, "git status")?.trim().is_empty())
}

fn command_unit(output: std::process::Output, context: &'static str) -> Result<()> {
    command_output(output, context).map(|_| ())
}

fn command_output(output: std::process::Output, context: &'static str) -> Result<String> {
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }
    Err(Error::Command {
        context,
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}
