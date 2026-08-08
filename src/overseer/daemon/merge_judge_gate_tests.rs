use super::*;
use crate::overseer::ledger::{LedgerPhase, OperatorOverride};

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
        merge_judge_primes: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        manual_merge_skip: None,
        merge_judge_fail_safes: 0,
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
    }
}

#[test]
fn only_the_two_waits_a_judgment_can_run_underneath_prime_one() {
    // Both of these name work that finishes on its own, so the change the judge
    // would read is the change the gate will clear. Running the judgment now is
    // what stops a merge costing a full check run and then a full judgment.
    assert!(waiting_on_progress(merge_gate::CHECKS_WAITING));
    assert!(waiting_on_progress(merge_state::BRANCH_UPDATED));
}

#[test]
fn a_fault_or_a_wait_on_another_pull_request_never_primes_a_judgment() {
    // A fault needs someone to change the pull request, and the judge would be
    // asked about a change that is about to be replaced. A wait on another pull
    // request means the one ahead in the queue is the one worth a judgment.
    for reason in [
        "checks_not_green",
        "merge_state:dirty",
        "merge_state:blocked",
        "merge_state:draft",
        crate::overseer::daemon::merge_queue::WAITING_TURN,
        crate::overseer::daemon::merge_state::UPDATE_CAP_REACHED,
        "unprotected:no_pull_request_rule",
        "prerequisite_unmerged:#42",
        "repo_merge_settling",
    ] {
        assert!(!waiting_on_progress(reason), "{reason} must not prime");
    }
}

#[test]
fn the_priming_budget_bounds_how_many_early_judgments_one_pull_request_buys() {
    // A judgment is keyed on the change, and `checks_waiting` follows every push,
    // so without this a worker pushing ten CI fixes would buy ten judgments where
    // waiting for green checks bought one. Running `daily_llm_budget` out does not
    // merely stop priming — it escalates every merge on the board for the rest of
    // the day — so this budget is the one that has to hold.
    let mut entry = base_entry(LedgerPhase::PrOpened);
    let mut config = Config::default();
    config.overseer.max_merge_judge_primes = 2;

    for spent in 0..2 {
        assert!(budget_left(&entry, &config), "look {spent} must be granted");
        // What `prime` does when `prime_merge` reports a judgment was started.
        entry.merge_judge_primes += 1;
    }
    assert!(!budget_left(&entry, &config));
}

#[test]
fn a_zero_budget_turns_early_judgments_off_entirely() {
    // The operator's route back to the old behaviour: every merge judgment runs
    // after the gate clears, and nothing is spent ahead of it.
    let entry = base_entry(LedgerPhase::PrOpened);
    let mut config = Config::default();
    config.overseer.max_merge_judge_primes = 0;

    assert!(!budget_left(&entry, &config));
    assert_eq!(entry.merge_judge_primes, 0);
}

#[test]
fn veto_escalates_and_cannot_be_selected_again_at_same_revision() {
    let mut entry = base_entry(LedgerPhase::PrOpened);
    assert!(!judgment_allows_merge(&mut entry, MergeJudgment::Veto));
    assert_eq!(entry.phase, LedgerPhase::Escalated);
    assert_ne!(entry.phase, LedgerPhase::PrOpened);
}

#[test]
fn take_operator_override_returns_false_and_leaves_the_entry_alone_when_none_is_pending() {
    let mut entry = base_entry(LedgerPhase::Escalated);
    assert!(!take_operator_override(&mut entry, "head1", "judge_veto:x").unwrap());
    assert!(entry.operator_override.is_none());
}

#[test]
fn take_operator_override_consumes_but_refuses_a_mismatched_head() {
    let mut entry = base_entry(LedgerPhase::Escalated);
    entry.operator_override = Some(OperatorOverride {
        head: "old-head".into(),
        granted_at: chrono::Utc::now(),
    });

    assert!(!take_operator_override(&mut entry, "new-head", "judge_veto:x").unwrap());

    // Taken either way: a stale grant is spent, not retried against a
    // revision it was never approved for.
    assert!(entry.operator_override.is_none());
}

#[test]
fn take_operator_override_bypasses_on_a_matching_head() {
    let mut entry = base_entry(LedgerPhase::Escalated);
    entry.operator_override = Some(OperatorOverride {
        head: "abc123".into(),
        granted_at: chrono::Utc::now(),
    });

    assert!(take_operator_override(&mut entry, "abc123", "judge_veto:nope").unwrap());

    assert!(entry.operator_override.is_none());
    // `take_operator_override` never touches `phase` itself — `judge_allows`
    // only calls `judgment_allows_merge` on the non-bypassed path, so a
    // bypassed entry stays whatever phase it already was instead of being
    // marked `Escalated` right before it merges.
    assert_eq!(entry.phase, LedgerPhase::Escalated);
}
