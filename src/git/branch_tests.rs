use std::path::Path;

use super::*;
use crate::git::test_repo::{TestRepo, git};

/// A `--merge` landing: the branch tip becomes an ancestor of the base.
#[test]
fn merge_commit_counts_as_merged() {
    let repo = TestRepo::new();
    repo.feature_branch("feature", "feature.txt");
    git(repo.path(), &["checkout", "-q", "main"]);
    git(
        repo.path(),
        &["merge", "-q", "--no-ff", "-m", "merge", "feature"],
    );
    assert!(merged(repo.path(), "feature"));
}

/// A `--squash` landing: the branch's commits are replaced by one new commit,
/// so nothing on the branch is an ancestor of the base and no commit of it has
/// a patch-equivalent twin either.
#[test]
fn squash_merge_counts_as_merged() {
    let repo = TestRepo::new();
    repo.feature_branch("feature", "feature.txt");
    repo.commit_file("feature", "second.txt", "second");
    git(repo.path(), &["checkout", "-q", "main"]);
    git(repo.path(), &["merge", "-q", "--squash", "feature"]);
    git(repo.path(), &["commit", "-qm", "squashed feature (#1)"]);
    assert!(!ancestor_of_main(repo.path(), "feature"));
    assert!(merged(repo.path(), "feature"));
}

/// A `--rebase` landing: each commit is replayed onto the base under a new id.
#[test]
fn rebased_commits_count_as_merged() {
    let repo = TestRepo::new();
    repo.feature_branch("feature", "feature.txt");
    repo.commit_file("main", "unrelated.txt", "unrelated");
    git(repo.path(), &["checkout", "-q", "main"]);
    git(repo.path(), &["cherry-pick", "feature"]);
    assert!(!ancestor_of_main(repo.path(), "feature"));
    assert!(merged(repo.path(), "feature"));
}

#[test]
fn unmerged_branch_is_not_merged() {
    let repo = TestRepo::new();
    repo.feature_branch("feature", "feature.txt");
    repo.commit_file("main", "unrelated.txt", "unrelated");
    assert!(!merged(repo.path(), "feature"));
}

/// A branch whose first commit landed but whose second did not stays unmerged —
/// the check is about the whole branch, not its cheapest part.
#[test]
fn partially_merged_branch_is_not_merged() {
    let repo = TestRepo::new();
    repo.feature_branch("feature", "feature.txt");
    git(repo.path(), &["checkout", "-q", "main"]);
    git(repo.path(), &["cherry-pick", "feature"]);
    repo.commit_file("feature", "second.txt", "second");
    assert!(!merged(repo.path(), "feature"));
}

fn merged(repo: &Path, branch: &str) -> bool {
    branch_content_merged(repo, branch, "main").unwrap()
}

fn ancestor_of_main(repo: &Path, branch: &str) -> bool {
    is_ancestor(repo, branch, "main").unwrap()
}
