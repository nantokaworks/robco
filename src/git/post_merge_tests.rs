use super::*;
use crate::git::{
    branch_exists,
    test_repo::{TestRepo, git},
};

/// The whole point of the task: after a squash merge the daemon's cleanup
/// removes the worktree and the branch, from knowing about `origin/main`
/// alone — it never needs the primary worktree itself to reach the merge
/// commit.
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
}

/// This is the fix itself: when the primary checkout has `main` checked out
/// with a clean tree, cleanup fast-forwards it — not just `origin/main` —
/// so the checkout stops trailing behind what actually landed.
#[test]
fn main_is_fast_forwarded_when_checked_out_and_clean() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    repo.push("task");
    let worktree = repo.worktree("task");
    repo.land_squash("task");

    let outcome = cleanup(&repo, &worktree, OnFailure::Continue)
        .run(|_| ())
        .unwrap();

    assert!(outcome.base_pulled);
    assert_eq!(outcome.notes, Vec::<String>::new());
    assert_eq!(current_branch(&repo), "main");
    assert_eq!(rev_parse(&repo, "main"), rev_parse(&repo, "origin/main"));
    assert!(repo.path().join("task.txt").exists());
}

/// Cleanup must never touch whatever the operator has checked out in the
/// primary worktree, uncommitted work included — knowing about `origin/main`
/// is enough to decide the branch's fate. It still advances the local `main`
/// ref, since a ref update touches neither the working tree nor `HEAD`.
#[test]
fn an_operators_dirty_checkout_survives_cleanup() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    repo.push("task");
    let worktree = repo.worktree("task");
    repo.land_squash("task");

    // The operator is mid-work on a branch of their own, uncommitted.
    git(repo.path(), &["checkout", "-qb", "operator-wip"]);
    std::fs::write(repo.path().join("wip.txt"), "not yet committed").unwrap();

    let outcome = cleanup(&repo, &worktree, OnFailure::Continue)
        .run(|_| ())
        .unwrap();

    assert!(outcome.base_pulled);
    assert_eq!(outcome.branch, BranchOutcome::Deleted);
    assert_eq!(current_branch(&repo), "operator-wip");
    assert_eq!(
        std::fs::read_to_string(repo.path().join("wip.txt")).unwrap(),
        "not yet committed"
    );
    // The squash landed on `origin/main`, but the primary worktree's own
    // files never moved off `operator-wip` to receive it.
    assert!(!repo.path().join("task.txt").exists());
    // Only the ref moved — `main` now points at the merge, even unchecked out.
    assert_eq!(rev_parse(&repo, "main"), rev_parse(&repo, "origin/main"));
}

/// `main` checked out with uncommitted changes is the one shape this step
/// cannot advance safely, so it is left exactly as it was and the drift is
/// recorded rather than silently dropped.
#[test]
fn a_dirty_main_checkout_is_left_behind_with_a_note() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    repo.push("task");
    let worktree = repo.worktree("task");
    let before = rev_parse(&repo, "main");
    repo.land_squash("task");
    std::fs::write(repo.path().join("f.txt"), "dirty").unwrap();
    git(repo.path(), &["add", "f.txt"]);

    let outcome = cleanup(&repo, &worktree, OnFailure::Continue)
        .run(|_| ())
        .unwrap();

    assert!(!outcome.base_pulled);
    assert_eq!(current_branch(&repo), "main");
    assert_eq!(rev_parse(&repo, "main"), before);
    assert_eq!(
        std::fs::read_to_string(repo.path().join("f.txt")).unwrap(),
        "dirty"
    );
    assert!(
        outcome.notes[0].contains("uncommitted changes") && outcome.notes[0].contains("behind"),
        "unexpected notes: {:?}",
        outcome.notes
    );
}

/// A local `main` that diverged (a commit `origin/main` never saw) cannot be
/// fast-forwarded either way, and both shapes refuse it on their own — the
/// left-behind commit is not silently discarded by a force update.
#[test]
fn a_diverged_local_main_is_left_behind_with_a_note() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    repo.push("task");
    let worktree = repo.worktree("task");
    repo.land_squash("task");
    std::fs::write(repo.path().join("local-only.txt"), "never pushed").unwrap();
    git(repo.path(), &["add", "local-only.txt"]);
    git(repo.path(), &["commit", "-qm", "local-only"]);
    let before = rev_parse(&repo, "main");

    let outcome = cleanup(&repo, &worktree, OnFailure::Continue)
        .run(|_| ())
        .unwrap();

    assert!(!outcome.base_pulled);
    assert_eq!(rev_parse(&repo, "main"), before);
    assert!(repo.path().join("local-only.txt").exists());
    assert!(
        outcome.notes[0].contains("behind"),
        "unexpected notes: {:?}",
        outcome.notes
    );
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

/// A base-branch fetch that cannot run must not strand the worktree: the
/// daemon has nobody to report to, so it logs and finishes the sequence.
#[test]
fn continue_records_a_failed_base_fetch_and_cleans_up_anyway() {
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
        outcome.notes[0].starts_with("fetching the base branch failed:"),
        "unexpected notes: {:?}",
        outcome.notes
    );
    // `origin/main` never advanced, so the landed change is not visible here
    // and the branch is held back rather than deleted on a stale answer.
    assert_eq!(outcome.branch, BranchOutcome::Kept);
}

/// The interactive path keeps its own contract: the first failure is the
/// caller's, and nothing after it runs.
#[test]
fn abort_stops_at_a_failed_base_fetch() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    let worktree = repo.worktree("task");
    git(repo.path(), &["remote", "remove", "origin"]);

    let error = cleanup(&repo, &worktree, OnFailure::Abort)
        .run(|_| ())
        .unwrap_err();

    assert!(
        error.to_string().contains("git fetch origin"),
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

fn current_branch(repo: &TestRepo) -> String {
    let output = std::process::Command::new("git")
        .args(["-C"])
        .arg(repo.path())
        .args(["symbolic-ref", "--short", "HEAD"])
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn rev_parse(repo: &TestRepo, reference: &str) -> String {
    let output = std::process::Command::new("git")
        .args(["-C"])
        .arg(repo.path())
        .args(["rev-parse", reference])
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}
