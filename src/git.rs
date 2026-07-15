use std::{
    path::{Path, PathBuf},
    process::Command,
};

use crate::{Error, Result};

/// A single entry from `git worktree list --porcelain`.
#[derive(Debug, Clone)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
}

pub fn list_worktrees(repo: &Path) -> Result<Vec<Worktree>> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["worktree", "list", "--porcelain"])
        .output()?;
    let text = command_output(output, "git worktree list")?;

    let mut worktrees = Vec::new();
    let mut current: Option<Worktree> = None;
    for line in text.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(worktree) = current.take() {
                worktrees.push(worktree);
            }
            current = Some(Worktree {
                path: PathBuf::from(path),
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

pub fn remove_worktree(repo: &Path, worktree: &Path, force: bool) -> Result<()> {
    let mut command = Command::new("git");
    command.args(["-C"]).arg(repo).args(["worktree", "remove"]);
    if force {
        command.arg("--force");
    }
    let output = command.arg(worktree).output()?;
    command_unit(output, "git worktree remove")?;
    prune_worktrees(repo)
}

pub fn prune_worktrees(repo: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["worktree", "prune"])
        .output()?;
    command_unit(output, "git worktree prune")
}

pub fn branch_exists(repo: &Path, branch: &str) -> Result<bool> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["show-ref", "--verify", "--quiet"])
        .arg(format!("refs/heads/{branch}"))
        .output()?;
    Ok(output.status.success())
}

pub fn delete_branch(repo: &Path, branch: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["branch", "-D", branch])
        .output()?;
    command_unit(output, "git branch -D")
}

pub fn delete_remote_branch(repo: &Path, branch: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["push", "origin", "--delete", branch])
        .output()?;
    command_unit(output, "git push origin --delete")
}

pub fn tracked_tree_is_clean(worktree: &Path) -> Result<bool> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(worktree)
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()?;
    Ok(command_output(output, "git status")?.trim().is_empty())
}

pub fn ahead_behind(repo: &Path, left: &str, right: &str) -> Result<(u32, u32)> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["rev-list", "--left-right", "--count"])
        .arg(format!("{left}...{right}"))
        .output()?;
    let text = command_output(output, "git rev-list --left-right --count")?;
    let mut counts = text.split_whitespace();
    let left = counts
        .next()
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| Error::Command {
            context: "git rev-list --left-right --count",
            stderr: "invalid count output".to_string(),
        })?;
    let right = counts
        .next()
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| Error::Command {
            context: "git rev-list --left-right --count",
            stderr: "invalid count output".to_string(),
        })?;
    Ok((left, right))
}

pub fn worktree_is_clean(worktree: &Path) -> Result<bool> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(worktree)
        .args(["status", "--porcelain"])
        .output()?;
    Ok(command_output(output, "git status")?.trim().is_empty())
}

pub fn status_short(worktree: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(worktree)
        .args(["status", "--short"])
        .output()?;
    command_output(output, "git status --short")
}

pub fn diff(worktree: &Path) -> Result<String> {
    let status = status_short(worktree)?;
    let stat = Command::new("git")
        .args(["-C"])
        .arg(worktree)
        .args(["diff", "--stat"])
        .output()?;
    let patch = Command::new("git")
        .args(["-C"])
        .arg(worktree)
        .args(["diff"])
        .output()?;

    let stat = command_output(stat, "git diff --stat")?;
    let patch = command_output(patch, "git diff")?;
    let diff = match (stat.trim().is_empty(), patch.trim().is_empty()) {
        (true, true) => "No tracked diff.".to_string(),
        (false, true) => stat,
        (true, false) => patch,
        (false, false) => format!("{stat}\n\n{patch}"),
    };

    if status.trim().is_empty() {
        Ok(diff)
    } else {
        Ok(format!("status:\n{status}\n\n{diff}"))
    }
}

pub fn pr_exists(repo: &Path, branch: &str) -> Result<bool> {
    let output = Command::new("gh")
        .current_dir(repo)
        .args(["pr", "list", "--head", branch, "--state", "open"])
        .args(["--json", "number"])
        .output()?;
    let output = command_output(output, "gh pr list")?;
    Ok(!matches!(output.trim(), "" | "[]"))
}

pub fn merge_pr(repo: &Path, branch: &str, strategy_flag: &str) -> Result<()> {
    let output = Command::new("gh")
        .current_dir(repo)
        .args(["pr", "merge", branch, strategy_flag])
        .output()?;
    command_unit(output, "gh pr merge")
}

pub fn pull_ff_only(main_worktree: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(main_worktree)
        .args(["pull", "--ff-only"])
        .output()?;
    command_unit(output, "git pull --ff-only")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn git(repo: &Path, args: &[&str]) {
        assert!(
            Command::new("git")
                .args(["-C"])
                .arg(repo)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }

    #[test]
    fn ahead_behind_parses_counts() {
        let temp = tempfile::tempdir().unwrap();
        git(temp.path(), &["init", "-q"]);
        git(temp.path(), &["config", "user.email", "test@example.com"]);
        git(temp.path(), &["config", "user.name", "Test"]);
        std::fs::write(temp.path().join("file"), "base").unwrap();
        git(temp.path(), &["add", "file"]);
        git(temp.path(), &["commit", "-qm", "base"]);
        git(temp.path(), &["branch", "left"]);
        git(temp.path(), &["checkout", "-qb", "right"]);
        std::fs::write(temp.path().join("file"), "right").unwrap();
        git(temp.path(), &["commit", "-qam", "right"]);
        git(temp.path(), &["checkout", "-q", "left"]);
        std::fs::write(temp.path().join("other"), "left").unwrap();
        git(temp.path(), &["add", "other"]);
        git(temp.path(), &["commit", "-qm", "left"]);
        assert_eq!(ahead_behind(temp.path(), "left", "right").unwrap(), (1, 1));
    }
}
