use std::{path::Path, process::Command};

use super::{
    GIT_LOCAL_TIMEOUT, GIT_WORKTREE_REMOVE_TIMEOUT, Worktree, command_output, command_unit,
};
use crate::{Result, exec::run_timeout};

pub fn list_worktrees(repo: &Path) -> Result<Vec<Worktree>> {
    let mut command = Command::new("git");
    command
        .args(["-C"])
        .arg(repo)
        .args(["worktree", "list", "--porcelain"]);
    let output = run_timeout(command, GIT_LOCAL_TIMEOUT)?;
    let text = command_output(output, "git worktree list")?;

    let mut worktrees = Vec::new();
    let mut current: Option<Worktree> = None;
    for line in text.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(worktree) = current.take() {
                worktrees.push(worktree);
            }
            current = Some(Worktree {
                path: path.into(),
                head: None,
                branch: None,
            });
        } else if let Some(head) = line.strip_prefix("HEAD ")
            && let Some(worktree) = current.as_mut()
        {
            worktree.head = Some(head.to_string());
        } else if let Some(branch) = line.strip_prefix("branch ")
            && let Some(worktree) = current.as_mut()
        {
            worktree.branch = Some(
                branch
                    .strip_prefix("refs/heads/")
                    .unwrap_or(branch)
                    .to_string(),
            );
        }
    }
    if let Some(worktree) = current.take() {
        worktrees.push(worktree);
    }
    Ok(worktrees)
}

fn worktree_add_command(repo: &Path, worktree: &Path, branch: &str, base: &str) -> Command {
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
    let command = worktree_add_command(repo, worktree, branch, base);
    let output = run_timeout(command, GIT_LOCAL_TIMEOUT)?;
    command_unit(output, "git worktree add")
}

pub fn remove_worktree(repo: &Path, worktree: &Path, force: bool) -> Result<()> {
    let mut command = Command::new("git");
    command.args(["-C"]).arg(repo).args(["worktree", "remove"]);
    if force {
        command.arg("--force");
    }
    command.arg(worktree);
    let output = run_timeout(command, GIT_WORKTREE_REMOVE_TIMEOUT)?;
    command_unit(output, "git worktree remove")?;
    prune_worktrees(repo)
}

pub fn prune_worktrees(repo: &Path) -> Result<()> {
    let mut command = Command::new("git");
    command.args(["-C"]).arg(repo).args(["worktree", "prune"]);
    let output = run_timeout(command, GIT_LOCAL_TIMEOUT)?;
    command_unit(output, "git worktree prune")
}
