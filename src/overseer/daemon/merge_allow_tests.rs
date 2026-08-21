use super::*;
use crate::git::test_repo::TestRepo;
use crate::overseer::ledger::{LedgerPhase, MergeApproval, OperatorOverride};

fn base_entry(phase: LedgerPhase) -> LedgerEntry {
    LedgerEntry {
        task_id: "task".into(),
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
    }
}

#[test]
fn take_operator_override_returns_false_and_leaves_the_entry_alone_when_none_is_pending() {
    let mut entry = base_entry(LedgerPhase::Escalated);
    assert!(!take_operator_override(&mut entry, "head1").unwrap());
    assert!(entry.operator_override.is_none());
}

#[test]
fn take_operator_override_consumes_but_refuses_a_mismatched_head() {
    let mut entry = base_entry(LedgerPhase::Escalated);
    entry.operator_override = Some(OperatorOverride {
        head: "old-head".into(),
        granted_at: chrono::Utc::now(),
    });

    assert!(!take_operator_override(&mut entry, "new-head").unwrap());

    // Taken either way: a stale grant is spent, not retried against a
    // revision it was never approved for.
    assert!(entry.operator_override.is_none());
}

#[test]
fn take_operator_override_confirms_on_a_matching_head() {
    let mut entry = base_entry(LedgerPhase::Escalated);
    entry.operator_override = Some(OperatorOverride {
        head: "abc123".into(),
        granted_at: chrono::Utc::now(),
    });

    assert!(take_operator_override(&mut entry, "abc123").unwrap());

    assert!(entry.operator_override.is_none());
    // `take_operator_override` never touches `phase` itself.
    assert_eq!(entry.phase, LedgerPhase::Escalated);
}

#[test]
fn take_merge_approval_returns_false_and_leaves_the_entry_alone_when_none_is_pending() {
    let mut entry = base_entry(LedgerPhase::PrOpened);
    assert!(!take_merge_approval(&mut entry, "head1").unwrap());
    assert!(entry.merge_approval.is_none());
}

#[test]
fn take_merge_approval_consumes_but_refuses_a_mismatched_head() {
    let mut entry = base_entry(LedgerPhase::PrOpened);
    entry.merge_approval = Some(MergeApproval {
        head: "old-head".into(),
        granted_at: chrono::Utc::now(),
    });

    assert!(!take_merge_approval(&mut entry, "new-head").unwrap());

    // Taken either way, and the drop itself is recorded (dropr:456): a
    // silently dropped approval would look like a merge that should
    // already have happened.
    assert!(entry.merge_approval.is_none());
    // dropr:534: the drop is also recorded on the entry itself, so the row
    // can say why — a fabricated "old-head"/"new-head" pair can never be a
    // fast-forward of each other, so this never qualifies to carry forward.
    assert!(
        entry
            .approval_dropped
            .as_deref()
            .is_some_and(|reason| reason.starts_with("merge_approval_dropped:"))
    );
}

#[test]
fn take_merge_approval_confirms_on_a_matching_head() {
    let mut entry = base_entry(LedgerPhase::PrOpened);
    entry.merge_approval = Some(MergeApproval {
        head: "abc123".into(),
        granted_at: chrono::Utc::now(),
    });

    assert!(take_merge_approval(&mut entry, "abc123").unwrap());

    assert!(entry.merge_approval.is_none());
    assert_eq!(entry.phase, LedgerPhase::PrOpened);
}

#[test]
fn a_matching_merge_approval_completes_the_requested_merge() {
    let mut entry = base_entry(LedgerPhase::PrOpened);
    entry.merge_approval = Some(MergeApproval {
        head: "abc123".into(),
        granted_at: chrono::Utc::now(),
    });
    let value = serde_json::json!({"headRefOid": "abc123"});

    let judgment = confirm_requested(&mut entry, &value).unwrap();

    assert!(matches!(judgment, Judgment::Allow));
    assert!(entry.merge_approval.is_none());
}

#[test]
fn a_matching_operator_override_completes_the_requested_merge() {
    let mut entry = base_entry(LedgerPhase::PrOpened);
    entry.operator_override = Some(OperatorOverride {
        head: "abc123".into(),
        granted_at: chrono::Utc::now(),
    });
    let value = serde_json::json!({"headRefOid": "abc123"});

    let judgment = confirm_requested(&mut entry, &value).unwrap();

    assert!(matches!(judgment, Judgment::Allow));
    assert!(entry.operator_override.is_none());
}

#[test]
fn a_pull_request_pushed_past_the_approved_head_stays_held_rather_than_merging() {
    // Neither field is live for the current head — the worker pushed a fix
    // after the operator approved an older revision. Nothing here is a
    // problem: the operator has to look and press `m` again.
    let mut entry = base_entry(LedgerPhase::PrOpened);
    entry.merge_approval = Some(MergeApproval {
        head: "old-head".into(),
        granted_at: chrono::Utc::now(),
    });
    let value = serde_json::json!({"headRefOid": "new-head"});

    let judgment = confirm_requested(&mut entry, &value).unwrap();

    match judgment {
        Judgment::Halt(halt) => assert_eq!(halt.reason, "merge_request_stale"),
        Judgment::Allow => panic!("expected a halt"),
    }
    assert!(entry.merge_approval.is_none());
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
