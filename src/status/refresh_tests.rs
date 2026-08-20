use crate::{discover, git::test_repo::TestRepo};

use super::*;

#[test]
fn refresh_main_drift_reports_how_far_local_main_trails_origin() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    repo.push("task");
    repo.land_squash("task");
    crate::git::fetch_branch(repo.path(), "main").unwrap();
    let mut node = discover::repo_node(repo.path().to_path_buf(), false);

    refresh_main_drift(&mut node);

    assert_eq!(node.main_behind_origin, Some(1));
}

/// dropr:503 — the same drift measurement, but for a repository whose
/// default branch is `master`, not `main`. Comparing against a
/// hardcoded `main` would find nothing to compare and silently report
/// no drift at all.
#[test]
fn refresh_main_drift_follows_a_master_default_repository() {
    let repo = TestRepo::new_with_default_branch("master");
    repo.feature_branch("task", "task.txt");
    repo.push("task");
    repo.land_squash("task");
    crate::git::fetch_branch(repo.path(), "master").unwrap();
    let mut node = discover::repo_node(repo.path().to_path_buf(), false);

    refresh_main_drift(&mut node);

    assert_eq!(node.main_behind_origin, Some(1));
}

#[test]
fn refresh_main_drift_is_none_when_main_is_current() {
    let repo = TestRepo::new();
    let mut node = discover::repo_node(repo.path().to_path_buf(), false);

    refresh_main_drift(&mut node);

    assert_eq!(node.main_behind_origin, None);
}

#[test]
fn refresh_checkout_branch_is_none_on_main() {
    let repo = TestRepo::new();
    let mut node = discover::repo_node(repo.path().to_path_buf(), false);

    refresh_checkout_branch(&mut node);

    assert_eq!(node.checkout_state, None);
}

/// dropr:503 acceptance criterion: a repository whose default branch is
/// `master` shows no checkout warning while sitting on `master`.
#[test]
fn refresh_checkout_branch_is_none_on_a_master_default_repository() {
    let repo = TestRepo::new_with_default_branch("master");
    let mut node = discover::repo_node(repo.path().to_path_buf(), false);

    refresh_checkout_branch(&mut node);

    assert_eq!(node.checkout_state, None);
}

#[test]
fn refresh_checkout_branch_names_another_branch() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    let mut node = discover::repo_node(repo.path().to_path_buf(), false);

    refresh_checkout_branch(&mut node);

    assert_eq!(
        node.checkout_state,
        Some(crate::model::CheckoutState::OtherBranch {
            current: "task".into(),
            default_branch: "main".into(),
        })
    );
}

#[test]
fn refresh_checkout_branch_reports_detached_head() {
    let repo = TestRepo::new();
    crate::git::test_repo::git(repo.path(), &["checkout", "-q", "--detach", "main"]);
    let mut node = discover::repo_node(repo.path().to_path_buf(), false);

    refresh_checkout_branch(&mut node);

    assert_eq!(
        node.checkout_state,
        Some(crate::model::CheckoutState::Detached {
            default_branch: "main".into(),
        })
    );
}

/// dropr:503 — a repository with no `origin` at all must warn that its
/// default branch is unknown, never silently assume `main`.
#[test]
fn refresh_checkout_branch_warns_when_the_default_branch_is_unresolved() {
    let temp = tempfile::tempdir().unwrap();
    crate::git::test_repo::git(temp.path(), &["init", "-q"]);
    let mut node = discover::repo_node(temp.path().to_path_buf(), false);

    refresh_checkout_branch(&mut node);

    assert_eq!(
        node.checkout_state,
        Some(crate::model::CheckoutState::DefaultBranchUnknown)
    );
}
