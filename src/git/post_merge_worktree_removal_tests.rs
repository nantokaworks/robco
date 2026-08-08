//! Tests for the dirty-worktree force-retry gate in
//! [`Cleanup::remove_worktree`] (dropr:TKxgioWtorh7MZPbTBseC). Split out of
//! `post_merge_tests.rs` to keep that file to the base-branch-advance and
//! branch-delete behavior it already covered.

use super::*;
use crate::git::{branch_exists, test_repo::TestRepo};

/// The fix itself: a worktree left dirty by an interrupted `git worktree
/// remove` (the shape a killed `GIT_WORKTREE_REMOVE_TIMEOUT` leaves behind)
/// is force-removed on the next pass rather than failing forever, and the
/// branch follows since its content already landed.
#[test]
fn a_dirty_worktree_of_a_merged_branch_is_force_removed_and_the_branch_follows() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    repo.push("task");
    let worktree = repo.worktree("task");
    repo.land_squash("task");
    // Simulate the half-deleted tree an interrupted removal leaves behind:
    // the worktree is left with modified content `git worktree remove`
    // (without `--force`) refuses to touch.
    std::fs::write(worktree.join("task.txt"), "half deleted").unwrap();

    let outcome = cleanup(&repo, &worktree, OnFailure::Continue)
        .run(|_| ())
        .unwrap();

    assert_eq!(outcome.notes, Vec::<String>::new());
    assert!(outcome.worktree_removed);
    assert_eq!(outcome.branch, BranchOutcome::Deleted);
    assert!(!worktree.exists());
    assert!(!branch_exists(repo.path(), "task").unwrap());
}

/// The guard on the fix above: a dirty worktree is only ever safe to force
/// past because the branch's content is provably already in the base. A
/// branch that never merged keeps its dirty worktree exactly as it was, with
/// the reason recorded, the same as any other worktree-removal failure.
#[test]
fn a_dirty_worktree_of_an_unmerged_branch_is_never_force_removed() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    let worktree = repo.worktree("task");
    std::fs::write(worktree.join("task.txt"), "uncommitted work").unwrap();

    let outcome = cleanup(&repo, &worktree, OnFailure::Continue)
        .run(|_| ())
        .unwrap();

    assert!(!outcome.worktree_removed);
    assert_eq!(outcome.branch, BranchOutcome::Kept);
    assert_eq!(outcome.notes.len(), 1);
    assert!(
        outcome.notes[0].starts_with("removing the worktree failed:")
            && outcome.notes[0].contains("contains modified or untracked files"),
        "unexpected notes: {:?}",
        outcome.notes
    );
    assert!(worktree.exists());
    assert_eq!(
        std::fs::read_to_string(worktree.join("task.txt")).unwrap(),
        "uncommitted work"
    );
    assert!(branch_exists(repo.path(), "task").unwrap());
}

fn cleanup<'a>(repo: &'a TestRepo, worktree: &'a Path, on_failure: OnFailure) -> Cleanup<'a> {
    Cleanup {
        repo: repo.path(),
        worktree,
        branch: "task",
        on_failure,
    }
}
