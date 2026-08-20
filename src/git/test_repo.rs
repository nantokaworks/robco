//! A throwaway repository with a real `origin`, for tests that need `git` to
//! answer honestly: ancestry, patch equivalence, fast-forwards, worktrees.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use tempfile::TempDir;

/// Runs `git` in `repo` and asserts it succeeded. The identity is passed per
/// invocation so the tests do not depend on the machine's git configuration.
pub(crate) fn git(repo: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    command
        .args([
            "-c",
            "user.name=robco",
            "-c",
            "user.email=robco@localhost",
            "-c",
            "commit.gpgsign=false",
            "-C",
        ])
        .arg(repo)
        .args(args);
    let output = command.output().expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(crate) struct TestRepo {
    temp: TempDir,
    repo: PathBuf,
    default_branch: String,
}

impl TestRepo {
    /// A bare `origin` plus a clone holding one commit on `main`, pushed.
    pub(crate) fn new() -> Self {
        Self::new_with_default_branch("main")
    }

    /// Same shape as [`Self::new`], but the bare `origin` (and so
    /// `refs/remotes/origin/HEAD` in the clone) is cut on `default_branch`
    /// instead of `main` — for tests that must prove behaviour follows the
    /// repository's own default branch rather than assuming `main`
    /// (dropr:503).
    pub(crate) fn new_with_default_branch(default_branch: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        git(
            &root,
            &["init", "-q", "--bare", "-b", default_branch, "origin.git"],
        );
        git(&root, &["clone", "-q", "origin.git", "repo"]);
        let repo = Self {
            repo: root.join("repo"),
            temp,
            default_branch: default_branch.to_string(),
        };
        std::fs::write(repo.path().join("base.txt"), "base").unwrap();
        git(repo.path(), &["add", "base.txt"]);
        git(repo.path(), &["commit", "-qm", "base"]);
        git(repo.path(), &["push", "-q", "-u", "origin", default_branch]);
        // The bare `origin` had no commits yet at clone time, so `git clone`
        // could not learn its `HEAD` then — `refs/remotes/origin/HEAD` is
        // left unset until something resolves it, exactly like a repository
        // an operator would repair with `git remote set-head origin -a`
        // (dropr:503). Do that repair here, now that `origin` actually has a
        // branch, so fixtures answer `git::default_branch` the way a real
        // clone would have.
        git(repo.path(), &["remote", "set-head", "origin", "-a"]);
        repo
    }

    pub(crate) fn path(&self) -> &Path {
        &self.repo
    }

    /// Branches off the default branch and commits one file. Leaves `HEAD`
    /// on the branch.
    pub(crate) fn feature_branch(&self, branch: &str, file: &str) {
        git(self.path(), &["checkout", "-q", &self.default_branch]);
        git(self.path(), &["checkout", "-qb", branch]);
        self.write_commit(file, file);
    }

    /// Commits one file on an existing branch. Leaves `HEAD` on that branch.
    pub(crate) fn commit_file(&self, branch: &str, file: &str, contents: &str) {
        git(self.path(), &["checkout", "-q", branch]);
        self.write_commit(file, contents);
    }

    /// Checks `branch` out into its own worktree, as a task agent would.
    pub(crate) fn worktree(&self, branch: &str) -> PathBuf {
        let worktree = self.temp.path().join(format!("wt-{branch}"));
        git(self.path(), &["checkout", "-q", &self.default_branch]);
        git(
            self.path(),
            &["worktree", "add", "-q", worktree.to_str().unwrap(), branch],
        );
        worktree
    }

    pub(crate) fn push(&self, branch: &str) {
        git(self.path(), &["push", "-q", "origin", branch]);
    }

    /// Squash-merges `branch` into `origin/main` from a second clone, the shape
    /// GitHub leaves behind: the changes land under a brand new commit and this
    /// clone's `main` is left one commit behind.
    pub(crate) fn land_squash(&self, branch: &str) {
        let root = self.temp.path();
        let lander = root.join("lander");
        if !lander.exists() {
            git(root, &["clone", "-q", "origin.git", "lander"]);
        }
        git(&lander, &["fetch", "-q", "origin"]);
        git(&lander, &["checkout", "-q", &self.default_branch]);
        git(
            &lander,
            &["merge", "-q", "--squash", &format!("origin/{branch}")],
        );
        git(&lander, &["commit", "-qm", &format!("squashed {branch}")]);
        git(&lander, &["push", "-q", "origin", &self.default_branch]);
    }

    fn write_commit(&self, file: &str, contents: &str) {
        std::fs::write(self.path().join(file), contents).unwrap();
        git(self.path(), &["add", file]);
        git(self.path(), &["commit", "-qm", &format!("add {file}")]);
    }
}
