//! `take_merge_approval`'s carry-forward rail (dropr:534, dropr:577): whether
//! a push that moved the pull request past its approved head is one of the
//! two robco-driven moves that keep the approval alive, split out of
//! `merge_allow_tests` because it needs a real git repository
//! (`TestRepo`) to exercise `is_descendant`'s fast-forward check.

use super::*;
use crate::git::test_repo::TestRepo;
use crate::overseer::ledger::{LedgerPhase, MergeApproval};

fn base_entry(phase: LedgerPhase) -> LedgerEntry {
    LedgerEntry {
        task_id: "task".into(),
        dropr_task_id: None,
        display_id: "#1".into(),
        repo: "/repo".into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
        phase,
        dispatched_at: chrono::Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: Some("https://pr/1".into()),
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        merge_hold_cap_escalated: false,
        merge_hold_rechecks: 0,
        merge_hold_recheck_reason: None,
        merge_hold_recheck_head: None,
        prerequisite_wait: None,
        merge_hold_stuck_notified: false,
        escalation_notified_reason: None,
        escalation_notified_head: None,
        worker_escalated: false,
        operator_override: None,
        merge_approval: None,
        pr_facts: None,
        worker_finished_at: None,
        approval_dropped: None,
        branch_update_head: None,
    }
}

/// A pending approval for `approved_head`, on `repo`'s `"task"` branch, with
/// `recovery_head` as the last head `merge_recovery` dispatched a handback
/// for (or `None` if it never did).
fn entry_awaiting(
    repo: &TestRepo,
    approved_head: &str,
    recovery_head: Option<&str>,
) -> LedgerEntry {
    let mut entry = base_entry(LedgerPhase::PrOpened);
    entry.repo = repo.path().to_string_lossy().into_owned();
    entry.branch = "task".into();
    entry.merge_approval = Some(MergeApproval {
        head: approved_head.to_owned(),
        granted_at: chrono::Utc::now(),
    });
    entry.merge_recovery.head = recovery_head.map(str::to_owned);
    entry
}

/// A pending approval for `approved_head`, on `repo`'s `"task"` branch, with
/// `update_head` as the head robco's own `BEHIND` branch update last recorded
/// (or `None` if it never ran one) — see [`entry_awaiting`] for the
/// recovery-dispatch counterpart.
fn entry_awaiting_update(
    repo: &TestRepo,
    approved_head: &str,
    update_head: Option<&str>,
) -> LedgerEntry {
    let mut entry = entry_awaiting(repo, approved_head, None);
    entry.branch_update_head = update_head.map(str::to_owned);
    entry
}

fn head(repo: &TestRepo) -> String {
    crate::git::local_branch_commit(repo.path(), "task").unwrap()
}

/// dropr:534: a worker's plain push, after robco itself dispatched a
/// recovery handback for the exact head the operator approved, is the fix
/// the approval was granted for — it survives under the new head instead of
/// being dropped.
#[test]
fn a_descendant_push_from_the_dispatched_recovery_carries_the_approval_forward() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "feature.txt");
    repo.push("task");
    let approved_head = head(&repo);

    // The worker's fix: one more commit on the same branch, pushed.
    repo.commit_file("task", "fix.txt", "fixed");
    repo.push("task");
    let new_head = head(&repo);

    let mut entry = entry_awaiting(&repo, &approved_head, Some(&approved_head));
    assert!(take_merge_approval(&mut entry, &new_head).unwrap());

    assert_eq!(
        entry.merge_approval.map(|approval| approval.head),
        Some(new_head)
    );
    assert!(entry.approval_dropped.is_none());
}

/// dropr:534: a force-push replaces the approved commit rather than building
/// on it, so the new head is never a descendant — the approval must drop
/// even though robco dispatched a recovery for the exact revision rewritten.
#[test]
fn a_force_push_after_recovery_still_drops_the_approval() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "feature.txt");
    repo.push("task");
    let approved_head = head(&repo);

    crate::git::test_repo::git(repo.path(), &["commit", "--amend", "-qm", "rewritten"]);
    crate::git::test_repo::git(repo.path(), &["push", "-q", "-f", "origin", "task"]);
    let new_head = head(&repo);

    let mut entry = entry_awaiting(&repo, &approved_head, Some(&approved_head));
    assert!(!take_merge_approval(&mut entry, &new_head).unwrap());
    assert!(entry.merge_approval.is_none());
    assert!(entry.approval_dropped.is_some());
}

/// dropr:534: a head git cannot place — never fetched, never existed — must
/// resolve to "not a descendant" rather than propagate an error. Treating an
/// ambiguous read as permission is exactly the unsafe default this must
/// never take.
#[test]
fn an_unresolvable_approved_head_drops_the_approval_instead_of_erroring() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "feature.txt");
    repo.push("task");
    let unknown = "0000000000000000000000000000000000dead";

    let mut entry = entry_awaiting(&repo, unknown, Some(unknown));
    assert!(!take_merge_approval(&mut entry, &head(&repo)).unwrap());
    assert!(entry.merge_approval.is_none());
    assert!(entry.approval_dropped.is_some());
}

/// dropr:534's own out-of-scope rail: a descendant push nobody at robco
/// dispatched a recovery for must not carry the approval forward, or it
/// becomes "any later commit on this branch" — the open-ended shape the task
/// explicitly rules out.
#[test]
fn a_descendant_push_with_no_matching_recovery_dispatch_still_drops() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "feature.txt");
    repo.push("task");
    let approved_head = head(&repo);

    repo.commit_file("task", "extra.txt", "unprompted");
    repo.push("task");
    let new_head = head(&repo);

    // No recovery was ever dispatched for this entry.
    let mut entry = entry_awaiting(&repo, &approved_head, None);
    assert!(!take_merge_approval(&mut entry, &new_head).unwrap());
    assert!(entry.merge_approval.is_none());
    assert!(entry.approval_dropped.is_some());
}

/// dropr:577: a `gh pr update-branch` robco ran for a `BEHIND` pull request
/// moves the head the same way a worker's own push does, but the move is
/// robco's own — the operator's approval survives onto the new head instead
/// of being dropped as if a worker had pushed unasked.
#[test]
fn a_descendant_push_from_robcos_own_branch_update_carries_the_approval_forward() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "feature.txt");
    repo.push("task");
    let approved_head = head(&repo);

    // The branch update: one more commit on the same branch, as
    // `gh pr update-branch` would produce.
    repo.commit_file("task", "base.txt", "caught up");
    repo.push("task");
    let new_head = head(&repo);

    let mut entry = entry_awaiting_update(&repo, &approved_head, Some(&approved_head));
    assert!(take_merge_approval(&mut entry, &new_head).unwrap());

    assert_eq!(
        entry.merge_approval.map(|approval| approval.head),
        Some(new_head)
    );
    assert!(entry.approval_dropped.is_none());
}

/// dropr:577: a force-push replaces the approved commit rather than building
/// on it, so the new head is never a descendant — the approval must drop
/// even though robco updated the branch onto the exact revision rewritten.
#[test]
fn a_force_push_after_a_branch_update_still_drops_the_approval() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "feature.txt");
    repo.push("task");
    let approved_head = head(&repo);

    crate::git::test_repo::git(repo.path(), &["commit", "--amend", "-qm", "rewritten"]);
    crate::git::test_repo::git(repo.path(), &["push", "-q", "-f", "origin", "task"]);
    let new_head = head(&repo);

    let mut entry = entry_awaiting_update(&repo, &approved_head, Some(&approved_head));
    assert!(!take_merge_approval(&mut entry, &new_head).unwrap());
    assert!(entry.merge_approval.is_none());
    assert!(entry.approval_dropped.is_some());
}
