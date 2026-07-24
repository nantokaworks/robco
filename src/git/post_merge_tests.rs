use super::*;
use crate::git::{
    branch_exists,
    test_repo::{TestRepo, git},
};

/// The whole point of the task: after a squash merge the daemon's cleanup
/// leaves the primary worktree at the merge commit and no branch behind.
#[test]
fn squash_merged_branch_and_worktree_are_removed() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    repo.push("task");
    let worktree = repo.worktree("task");
    repo.land_squash("task");

    let mut steps = Vec::new();
    let outcome = cleanup(&repo, &worktree, OnFailure::Continue)
        .run(|step| steps.push(step))
        .unwrap();

    assert_eq!(
        steps,
        vec![CleanupStep::PullingMain, CleanupStep::CleaningUp]
    );
    assert_eq!(outcome.notes, Vec::<String>::new());
    assert!(outcome.worktree_removed);
    assert_eq!(outcome.branch, BranchOutcome::Deleted);
    assert!(!worktree.exists());
    assert!(!branch_exists(repo.path(), "task").unwrap());
    assert!(head_contains(&repo, "task.txt"));
}

/// A branch whose changes never landed is the one case where deleting would
/// lose work, so it survives — with the reason recorded.
#[test]
fn unmerged_branch_is_kept_with_a_reason() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    let worktree = repo.worktree("task");

    let outcome = cleanup(&repo, &worktree, OnFailure::Continue)
        .run(|_| ())
        .unwrap();

    assert!(outcome.worktree_removed);
    assert_eq!(outcome.branch, BranchOutcome::Kept);
    assert_eq!(
        outcome.notes,
        vec!["branch task kept: its changes are not in the base branch".to_string()]
    );
    assert!(branch_exists(repo.path(), "task").unwrap());
}

/// A fast-forward that cannot run must not strand the worktree: the daemon has
/// nobody to report to, so it logs and finishes the sequence.
#[test]
fn continue_records_a_failed_fast_forward_and_cleans_up_anyway() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    repo.push("task");
    let worktree = repo.worktree("task");
    repo.land_squash("task");
    git(repo.path(), &["remote", "remove", "origin"]);

    let outcome = cleanup(&repo, &worktree, OnFailure::Continue)
        .run(|_| ())
        .unwrap();

    assert!(outcome.worktree_removed);
    assert!(!worktree.exists());
    assert!(
        outcome.notes[0].starts_with("fast-forwarding the primary worktree failed:"),
        "unexpected notes: {:?}",
        outcome.notes
    );
    // `main` never advanced, so the landed change is not visible here and the
    // branch is held back rather than deleted on a stale answer.
    assert_eq!(outcome.branch, BranchOutcome::Kept);
}

/// The interactive path keeps its own contract: the first failure is the
/// caller's, and nothing after it runs.
#[test]
fn abort_stops_at_a_failed_fast_forward() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    let worktree = repo.worktree("task");
    git(repo.path(), &["remote", "remove", "origin"]);

    let error = cleanup(&repo, &worktree, OnFailure::Abort)
        .run(|_| ())
        .unwrap_err();

    assert!(
        error.to_string().contains("git pull --ff-only"),
        "unexpected error: {error}"
    );
    assert!(worktree.exists());
    assert!(branch_exists(repo.path(), "task").unwrap());
}

/// A worktree removed by hand earlier is not a failure — the rest of the
/// cleanup still owes the branch a decision.
#[test]
fn missing_worktree_still_deletes_the_branch() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    repo.push("task");
    git(repo.path(), &["checkout", "-q", "main"]);
    repo.land_squash("task");
    let worktree = repo.path().join("never-created");

    let outcome = cleanup(&repo, &worktree, OnFailure::Continue)
        .run(|_| ())
        .unwrap();

    assert!(outcome.worktree_removed);
    assert_eq!(outcome.branch, BranchOutcome::Deleted);
}

fn cleanup<'a>(repo: &'a TestRepo, worktree: &'a Path, on_failure: OnFailure) -> Cleanup<'a> {
    Cleanup {
        repo: repo.path(),
        worktree,
        branch: "task",
        on_failure,
    }
}

fn head_contains(repo: &TestRepo, file: &str) -> bool {
    repo.path().join(file).exists()
}
