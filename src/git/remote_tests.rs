use super::*;
use crate::git::test_repo::TestRepo;

/// The point of routing dispatch and cleanup through this function rather
/// than a plain `git pull`: it learns the remote branch's commit without
/// ever moving whatever the repository has checked out.
#[test]
fn remote_branch_commit_fetches_without_touching_the_checkout() {
    let repo = TestRepo::new();
    let branch_before = branch_name(repo.path());

    let commit = remote_branch_commit(repo.path(), "main").unwrap();

    assert_eq!(commit, rev_parse(repo.path(), "origin/main"));
    assert_eq!(branch_name(repo.path()), branch_before);
}

fn branch_name(repo: &Path) -> String {
    let output = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["symbolic-ref", "--short", "HEAD"])
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn rev_parse(repo: &Path, reference: &str) -> String {
    let output = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(["rev-parse", reference])
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

#[test]
fn a_branch_without_pull_requests_is_absent() {
    assert_eq!(pr_state_from_list("[]").unwrap(), PrState::Absent);
    assert_eq!(pr_state_from_list("").unwrap(), PrState::Absent);
}

#[test]
fn each_terminal_state_is_distinguished() {
    assert_eq!(
        pr_state_from_list(r#"[{"state":"MERGED"}]"#).unwrap(),
        PrState::Merged
    );
    assert_eq!(
        pr_state_from_list(r#"[{"state":"CLOSED"}]"#).unwrap(),
        PrState::ClosedUnmerged
    );
    assert_eq!(
        pr_state_from_list(r#"[{"state":"OPEN"}]"#).unwrap(),
        PrState::Open
    );
}

#[test]
fn an_open_pull_request_outranks_earlier_attempts() {
    assert_eq!(
        pr_state_from_list(r#"[{"state":"CLOSED"},{"state":"OPEN"},{"state":"MERGED"}]"#).unwrap(),
        PrState::Open
    );
}

#[test]
fn a_merge_outranks_an_abandoned_attempt() {
    assert_eq!(
        pr_state_from_list(r#"[{"state":"CLOSED"},{"state":"MERGED"}]"#).unwrap(),
        PrState::Merged
    );
}

/// Unreadable output must not read as "no pull request": that is the one
/// answer that tells the user to open one they may already have.
#[test]
fn unreadable_output_is_an_error() {
    assert!(pr_state_from_list("not json").is_err());
}

#[test]
fn default_branch_reads_the_clones_own_origin_head() {
    let repo = TestRepo::new();
    assert_eq!(
        default_branch(repo.path()).unwrap().as_deref(),
        Some("main")
    );
}

/// The whole point of dropr:503: a repository cloned from a `master`
/// remote must answer `master`, never a hardcoded `main`.
#[test]
fn default_branch_follows_a_master_default_repository() {
    let repo = TestRepo::new_with_default_branch("master");
    assert_eq!(
        default_branch(repo.path()).unwrap().as_deref(),
        Some("master")
    );
}

/// A repository with no `origin` at all — created locally with `git
/// init`, never cloned — has no `origin/HEAD` to read. This must answer
/// `None`, not a guessed branch name.
#[test]
fn default_branch_is_none_without_a_remote() {
    let temp = tempfile::tempdir().unwrap();
    Command::new("git")
        .args(["init", "-q"])
        .current_dir(temp.path())
        .status()
        .unwrap();
    assert_eq!(default_branch(temp.path()).unwrap(), None);
}
