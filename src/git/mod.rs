mod branch;
mod merge_failure;
pub mod merge_flow;
// Crate-visible rather than private: the Overseer daemon's post-merge cleanup
// takes the same lock from outside this module.
pub(crate) mod merge_lock;
pub mod post_merge;
mod remote;
#[cfg(test)]
pub(crate) mod test_repo;
mod worktree;

pub use branch::*;
pub use merge_failure::*;
pub use remote::*;
pub use worktree::*;

use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
    time::Duration,
};

use crate::{Error, Result, exec::run_timeout};

pub(super) const GIT_LOCAL_TIMEOUT: Duration = Duration::from_secs(15);
pub(super) const GIT_NETWORK_TIMEOUT: Duration = Duration::from_secs(120);
/// `git worktree remove` walks and deletes every file in the tree, which can
/// run well past `GIT_LOCAL_TIMEOUT` on a large worktree. A removal that gets
/// killed mid-delete leaves a half-removed tree behind that no plain retry
/// can clean up (dropr:TKxgioWtorh7MZPbTBseC), so the removal gets a timeout
/// of its own rather than sharing the one sized for quick plumbing commands.
pub(super) const GIT_WORKTREE_REMOVE_TIMEOUT: Duration = Duration::from_secs(120);

/// A single entry from `git worktree list --porcelain`.
#[derive(Debug, Clone)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
}

pub fn remote_url(repo: &Path) -> Result<String> {
    let mut command = Command::new("git");
    command
        .args(["-C"])
        .arg(repo)
        .args(["remote", "get-url", "origin"]);
    let output = run_timeout(command, GIT_LOCAL_TIMEOUT)?;
    command_output(output, "git remote get-url origin")
}

pub fn branch_exists(repo: &Path, branch: &str) -> Result<bool> {
    let mut command = Command::new("git");
    command
        .args(["-C"])
        .arg(repo)
        .args(["show-ref", "--verify", "--quiet"])
        .arg(format!("refs/heads/{branch}"));
    let output = run_timeout(command, GIT_LOCAL_TIMEOUT)?;
    Ok(output.status.success())
}

pub fn delete_branch(repo: &Path, branch: &str) -> Result<()> {
    let mut command = Command::new("git");
    command
        .args(["-C"])
        .arg(repo)
        .args(["branch", "-D", branch]);
    let output = run_timeout(command, GIT_LOCAL_TIMEOUT)?;
    command_unit(output, "git branch -D")
}

pub fn tracked_tree_is_clean(worktree: &Path) -> Result<bool> {
    let mut command = Command::new("git");
    command
        .args(["-C"])
        .arg(worktree)
        .args(["status", "--porcelain", "--untracked-files=no"]);
    let output = run_timeout(command, GIT_LOCAL_TIMEOUT)?;
    Ok(command_output(output, "git status")?.trim().is_empty())
}

pub fn ahead_behind(repo: &Path, left: &str, right: &str) -> Result<(u32, u32)> {
    let mut command = Command::new("git");
    command
        .args(["-C"])
        .arg(repo)
        .args(["rev-list", "--left-right", "--count"])
        .arg(format!("{left}...{right}"));
    let output = run_timeout(command, GIT_LOCAL_TIMEOUT)?;
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
    let mut command = Command::new("git");
    command
        .args(["-C"])
        .arg(worktree)
        .args(["status", "--porcelain"]);
    let output = run_timeout(command, GIT_LOCAL_TIMEOUT)?;
    Ok(command_output(output, "git status")?.trim().is_empty())
}

pub fn status_short(worktree: &Path) -> Result<String> {
    let mut command = Command::new("git");
    command
        .args(["-C"])
        .arg(worktree)
        .args(["status", "--short"]);
    let output = run_timeout(command, GIT_LOCAL_TIMEOUT)?;
    command_output(output, "git status --short")
}

pub fn diff(worktree: &Path) -> Result<String> {
    let status = status_short(worktree)?;

    let mut stat_command = Command::new("git");
    stat_command
        .args(["-C"])
        .arg(worktree)
        .args(["diff", "--stat"]);
    let stat = run_timeout(stat_command, GIT_LOCAL_TIMEOUT)?;

    let mut patch_command = Command::new("git");
    patch_command.args(["-C"]).arg(worktree).arg("diff");
    let patch = run_timeout(patch_command, GIT_LOCAL_TIMEOUT)?;

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

pub(crate) fn command_unit(output: Output, context: &'static str) -> Result<()> {
    command_output(output, context).map(|_| ())
}

fn command_output(output: Output, context: &'static str) -> Result<String> {
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
        let mut command = Command::new("git");
        command.args(["-C"]).arg(repo).args(args);
        assert!(
            run_timeout(command, GIT_LOCAL_TIMEOUT)
                .unwrap()
                .status
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
