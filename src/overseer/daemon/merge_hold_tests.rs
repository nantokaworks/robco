use super::*;
use crate::overseer::ledger::LedgerPhase;

fn entry() -> LedgerEntry {
    LedgerEntry {
        task_id: "task".into(),
        display_id: "#1".into(),
        repo: "/repo".into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
        phase: LedgerPhase::PrOpened,
        dispatched_at: chrono::Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: Some("https://pr/1".into()),
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        manual_merge_skip: None,
        merge_judge_fail_safes: 0,
    }
}

/// The pattern that prompted the budget: 423 identical `unprotected` holds over
/// seven hours, none of which changed anything the board could show.
#[test]
fn a_frozen_reason_escalates_once_and_then_goes_quiet() {
    let mut entry = entry();
    let halt = Halt::gated("unprotected:no_pull_request_rule");

    for pass in 1..=3 {
        assert_eq!(charge(&mut entry, &halt, "sha-1", 3), HoldPlan::Record);
        assert_eq!(entry.merge_hold.passes, pass);
    }
    assert_eq!(charge(&mut entry, &halt, "sha-1", 3), HoldPlan::CapReached);
    // Every later pass is silent: an escalation restated once per poll is the
    // repetition this budget exists to end.
    for _ in 0..5 {
        assert_eq!(charge(&mut entry, &halt, "sha-1", 3), HoldPlan::Spent);
    }
    assert_eq!(entry.merge_hold.passes, 3);
}

#[test]
fn a_new_head_and_a_new_reason_each_restart_the_count() {
    let mut entry = entry();
    let waiting = Halt::hold("checks_waiting");
    assert_eq!(charge(&mut entry, &waiting, "sha-1", 2), HoldPlan::Record);
    assert_eq!(charge(&mut entry, &waiting, "sha-1", 2), HoldPlan::Record);

    // The worker pushed: whatever the old head was held on is no longer the
    // question being asked.
    assert_eq!(charge(&mut entry, &waiting, "sha-2", 2), HoldPlan::Record);
    assert_eq!(entry.merge_hold.passes, 1);
    assert_eq!(entry.merge_hold.head.as_deref(), Some("sha-2"));

    // Same revision, different problem.
    let dirty = Halt::hold("merge_state:dirty");
    assert_eq!(charge(&mut entry, &waiting, "sha-2", 2), HoldPlan::Record);
    assert_eq!(charge(&mut entry, &dirty, "sha-2", 2), HoldPlan::Record);
    assert_eq!(entry.merge_hold.passes, 1);
    assert_eq!(
        entry.merge_hold.reason.as_deref(),
        Some("merge_state:dirty")
    );
}

/// A pair that escalated and then changed is a new condition, not the old one
/// still being suppressed.
#[test]
fn an_escalated_pair_that_changes_is_recorded_again() {
    let mut entry = entry();
    let dirty = Halt::hold("merge_state:dirty");
    assert_eq!(charge(&mut entry, &dirty, "sha-1", 0), HoldPlan::CapReached);
    assert_eq!(charge(&mut entry, &dirty, "sha-1", 0), HoldPlan::Spent);

    assert_eq!(charge(&mut entry, &dirty, "sha-2", 1), HoldPlan::Record);
    assert!(!entry.merge_hold.escalated);
}

#[test]
fn a_zero_budget_escalates_the_first_held_pass() {
    let mut entry = entry();
    assert_eq!(
        charge(&mut entry, &Halt::hold("checks_waiting"), "sha-1", 0),
        HoldPlan::CapReached
    );
    assert_eq!(entry.merge_hold.passes, 0);
}

/// One condition must not spend two budgets: `behind_*` is already bounded by
/// `max_branch_updates`, and a settled pull request leaves the entry terminal.
#[test]
fn the_exits_that_carry_their_own_budget_are_never_charged() {
    let mut entry = entry();
    let exempt = [
        Halt::hold(super::super::merge_state::BRANCH_UPDATED),
        Halt::escalate(super::super::merge_state::UPDATE_CAP_REACHED),
        Halt::hold("behind_update_exit:exit status: 1"),
        Halt::skip("pr_already_merged"),
    ];
    for halt in &exempt {
        for _ in 0..4 {
            assert_eq!(
                charge(&mut entry, halt, "sha-1", 1),
                HoldPlan::Record,
                "expected exempt: {}",
                halt.reason
            );
        }
    }
    assert_eq!(entry.merge_hold, MergeHold::default());
}

/// An entry that clears the gate keeps no residue, so a later hold starts from a
/// full budget rather than from whatever the previous condition had spent.
#[test]
fn clearing_the_gate_forgets_what_the_entry_was_held_on() {
    let mut entry = entry();
    let waiting = Halt::hold("checks_waiting");
    assert_eq!(charge(&mut entry, &waiting, "sha-1", 2), HoldPlan::Record);
    assert_eq!(charge(&mut entry, &waiting, "sha-1", 2), HoldPlan::Record);

    cleared(&mut entry);
    assert_eq!(entry.merge_hold, MergeHold::default());
    assert_eq!(charge(&mut entry, &waiting, "sha-1", 2), HoldPlan::Record);
    assert_eq!(entry.merge_hold.passes, 1);
}

#[test]
fn the_cap_reason_names_what_ran_out() {
    // The decision log is where an operator finds out why a pull request stopped,
    // so "held too long" alone would not be actionable.
    assert_eq!(
        cap_reached("merge_state:dirty"),
        "merge_hold_cap_reached:merge_state:dirty"
    );
}
