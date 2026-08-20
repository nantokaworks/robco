use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{Error, Result, clone, git, registry::Registry};

/// What happened after a repository directory moved. Building this always
/// means the physical move already succeeded — a failure before that point
/// returns `Err` instead, with nothing touched.
#[derive(Debug)]
pub struct RenameOutcome {
    pub new_path: PathBuf,
    /// Worktree path, error text, for each linked worktree `git worktree
    /// repair` could not fix. Empty means every worktree repaired cleanly.
    pub unrepaired_worktrees: Vec<(PathBuf, String)>,
}

/// Renames a registered repository's own directory to `new_name`, a sibling
/// of its current directory, and repairs every linked worktree's back-link to
/// it. Does not touch the registry — callers apply the result with
/// [`apply_rename`].
///
/// Order matters: everything that can be checked without touching disk runs
/// first, so a validation failure leaves nothing changed. The single
/// `fs::rename` is the only step that can leave the repository half-moved;
/// everything after it is best-effort and reported back in
/// [`RenameOutcome::unrepaired_worktrees`] rather than turned into an `Err`,
/// since by then the directory has already moved and undoing that would be
/// its own risky operation.
pub fn rename_repo_dir(old_path: &Path, new_name: &str) -> Result<RenameOutcome> {
    if !clone::is_plain_component(new_name) {
        return Err(Error::Command {
            context: "repo rename",
            stderr: format!(
                "invalid repository name {new_name:?}: expected a single plain path component"
            ),
        });
    }
    // Canonicalized up front: `git worktree list` reports canonical paths
    // (e.g. resolving a `/var` -> `/private/var` symlink on macOS), and
    // comparing that against a non-canonical `old_path` would fail to
    // recognize the main worktree's own entry as itself, leaving it in the
    // repair list as a worktree that no longer exists once the rename below
    // moves it away.
    let old_path = old_path.canonicalize()?;
    let parent = old_path.parent().ok_or_else(|| Error::Command {
        context: "repo rename",
        stderr: format!("{} has no parent directory", old_path.display()),
    })?;
    let new_path = parent.join(new_name);
    if new_path == old_path {
        return Err(Error::Command {
            context: "repo rename",
            stderr: "new name is the same as the current name".to_string(),
        });
    }
    if new_path.exists() {
        return Err(Error::Command {
            context: "repo rename",
            stderr: format!("destination already exists: {}", new_path.display()),
        });
    }

    // Snapshot linked worktrees before anything moves, so a failure here
    // aborts cleanly with nothing changed. The main worktree itself is
    // excluded — it is `old_path`, about to move by the rename below, not
    // something `git worktree repair` acts on.
    let worktrees: Vec<PathBuf> = git::list_worktrees(&old_path)?
        .into_iter()
        .map(|worktree| worktree.path)
        .filter(|path| path != &old_path)
        .collect();

    fs::rename(&old_path, &new_path)?;

    // Repaired one call per worktree, not one batched `git worktree repair
    // <w1> <w2> ...` call, so a failure on one worktree cannot be mistaken
    // for a failure on another and cannot hide the ones that did succeed.
    let mut unrepaired = Vec::new();
    for worktree in worktrees {
        if let Err(error) = git::repair_worktree(&new_path, &worktree) {
            unrepaired.push((worktree, error.to_string()));
        }
    }

    Ok(RenameOutcome {
        new_path,
        unrepaired_worktrees: unrepaired,
    })
}

/// Applies a completed rename to a loaded registry, matching the row by its
/// pre-rename path. Returns whether a matching row was found — `false` means
/// the directory moved on disk but the registry has nothing to update, which
/// the caller must surface rather than treat as success.
pub fn apply_rename(
    registry: &mut Registry,
    old_path: &Path,
    new_path: &Path,
    new_name: &str,
) -> bool {
    let Some(repo) = registry.repos.iter_mut().find(|repo| repo.path == old_path) else {
        return false;
    };
    repo.path = new_path.to_path_buf();
    repo.name = new_name.to_string();
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::test_repo::TestRepo;

    #[test]
    fn renames_the_directory_and_repairs_a_linked_worktree() {
        let repo = TestRepo::new();
        repo.feature_branch("feature", "feature.txt");
        let worktree = repo.worktree("feature");

        let outcome = rename_repo_dir(repo.path(), "renamed").unwrap();

        assert!(outcome.unrepaired_worktrees.is_empty());
        assert_eq!(outcome.new_path.file_name().unwrap(), "renamed");
        assert!(!repo.path().exists());
        assert!(outcome.new_path.exists());
        // The worktree's `.git` back-link now resolves against the new path:
        // a plain git status must work without any manual repair.
        assert!(git::worktree_is_clean(&worktree).unwrap());
    }

    #[test]
    fn rejects_unsafe_new_names() {
        let repo = TestRepo::new();
        for name in ["../evil", "/tmp/evil", "nested/evil"] {
            let error = rename_repo_dir(repo.path(), name).unwrap_err();
            assert!(
                error.to_string().contains("single plain path component"),
                "{name}"
            );
        }
        assert!(repo.path().exists());
    }

    #[test]
    fn refuses_when_the_destination_already_exists() {
        let repo = TestRepo::new();
        let sibling = repo.path().parent().unwrap().join("taken");
        fs::create_dir(&sibling).unwrap();

        let error = rename_repo_dir(repo.path(), "taken").unwrap_err();

        assert!(error.to_string().contains("already exists"));
        assert!(repo.path().exists());
    }

    #[test]
    fn apply_rename_updates_the_matching_row_by_old_path() {
        let mut registry = Registry {
            version: 1,
            repos: vec![crate::discover::repo_node(
                PathBuf::from("/repos/old"),
                true,
            )],
        };
        let new_path = PathBuf::from("/repos/new");

        let applied = apply_rename(&mut registry, Path::new("/repos/old"), &new_path, "new");

        assert!(applied);
        assert_eq!(registry.repos[0].path, new_path);
        assert_eq!(registry.repos[0].name, "new");
    }

    #[test]
    fn apply_rename_reports_no_match_for_an_unknown_path() {
        let mut registry = Registry {
            version: 1,
            repos: Vec::new(),
        };

        let applied = apply_rename(
            &mut registry,
            Path::new("/repos/old"),
            Path::new("/repos/new"),
            "new",
        );

        assert!(!applied);
    }
}
