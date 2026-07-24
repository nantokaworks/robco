//! Whether a branch's changes already live somewhere else.

use std::{path::Path, process::Command};

use super::{GIT_LOCAL_TIMEOUT, command_output};
use crate::{Error, Result, exec::run_timeout};

/// Identity for the throwaway commit built by [`squash_landed`]. The commit is
/// never referenced, so the values only have to exist — `git commit-tree`
/// refuses to run without them and the repository's own identity may be unset.
const PROBE_IDENTITY: [&str; 4] = ["-c", "user.name=robco", "-c", "user.email=robco@localhost"];

/// Whether every change on `branch` is already present in `base`.
///
/// Ancestry is not a usable test on its own. A squash merge rewrites the branch
/// into a single new commit on `base`, so the branch tip is not an ancestor of
/// `base` and `git branch -d` refuses to delete it. Each merge strategy leaves
/// the content behind in a different shape, so all three shapes are tried:
///
/// 1. `--merge` — the branch tip is an ancestor of `base`.
/// 2. `--rebase` — every commit of the branch was replayed onto `base`, so each
///    one has a patch-equivalent commit there.
/// 3. `--squash` — the branch collapsed into one commit on `base`, so only the
///    branch *as a whole* has a patch-equivalent commit. Replaying the branch
///    tree as a single commit on the merge base reconstructs that shape, which
///    `git cherry` can then match.
pub fn branch_content_merged(repo: &Path, branch: &str, base: &str) -> Result<bool> {
    if is_ancestor(repo, branch, base)? {
        return Ok(true);
    }
    if commits_landed(repo, base, branch)? {
        return Ok(true);
    }
    squash_landed(repo, base, branch)
}

fn is_ancestor(repo: &Path, commit: &str, base: &str) -> Result<bool> {
    let mut command = Command::new("git");
    command
        .args(["-C"])
        .arg(repo)
        .args(["merge-base", "--is-ancestor", commit, base]);
    let output = run_timeout(command, GIT_LOCAL_TIMEOUT)?;
    match output.status.code() {
        Some(0) => Ok(true),
        // Exit 1 is the answer "no", every other exit is a broken invocation.
        Some(1) => Ok(false),
        _ => Err(Error::Command {
            context: "git merge-base --is-ancestor",
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        }),
    }
}

/// Whether each commit of `branch` has a patch-equivalent commit in `base`.
fn commits_landed(repo: &Path, base: &str, branch: &str) -> Result<bool> {
    let cherry = cherry(repo, base, branch)?;
    // `git cherry` prefixes a commit with `+` when nothing in `base` matches it.
    Ok(!cherry.lines().any(|line| line.starts_with('+')))
}

/// Whether the branch, taken as one change, has a patch-equivalent commit in
/// `base` — the shape a squash merge produces.
fn squash_landed(repo: &Path, base: &str, branch: &str) -> Result<bool> {
    let merge_base = git(repo, &["merge-base", base, branch], "git merge-base")?;
    let tree = git(
        repo,
        &["rev-parse", &format!("{branch}^{{tree}}")],
        "git rev-parse tree",
    )?;
    let mut command = Command::new("git");
    command.args(PROBE_IDENTITY).args(["-C"]).arg(repo).args([
        "commit-tree",
        &tree,
        "-p",
        &merge_base,
        "-m",
        "probe",
    ]);
    let probe = command_output(
        run_timeout(command, GIT_LOCAL_TIMEOUT)?,
        "git commit-tree probe",
    )?;
    let cherry = cherry(repo, base, &probe)?;
    // The probe commit is unreachable from `base`, so it is always listed; `-`
    // means `base` holds the same change under a different commit.
    Ok(cherry.lines().any(|line| line.starts_with('-')))
}

fn cherry(repo: &Path, base: &str, head: &str) -> Result<String> {
    git(repo, &["cherry", base, head], "git cherry")
}

fn git(repo: &Path, args: &[&str], context: &'static str) -> Result<String> {
    let mut command = Command::new("git");
    command.args(["-C"]).arg(repo).args(args);
    command_output(run_timeout(command, GIT_LOCAL_TIMEOUT)?, context)
}

#[cfg(test)]
#[path = "branch_tests.rs"]
mod tests;
