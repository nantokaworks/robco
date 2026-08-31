use super::*;

fn entry() -> LedgerEntry {
    LedgerEntry {
        task_id: "task".into(),
        dropr_task_id: None,
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

const REASON: &str = "checks_not_green";
const HEAD_A: &str = "aaaaaaa";
const HEAD_B: &str = "bbbbbbb";

/// The full shape this module exists for: the hold cap escalates the entry, a
/// condition that keeps changing spends the bounded looks, and once the
/// condition actually clears the caller settles the marker on the merge.
#[test]
fn cap_reached_then_condition_cleared_then_merged() {
    let mut entry = entry();
    entry.phase = LedgerPhase::Escalated;

    escalated(&mut entry, REASON, HEAD_A);
    assert!(entry.merge_hold_cap_escalated);
    assert_eq!(entry.merge_hold_rechecks, 0);

    // The very next pass is given a fresh look — no operator hand-merge needed,
    // and no waiting for a new head sha.
    assert!(due(&entry, 3));
    // A worker pushed a new revision, still failing: a genuine change, so it
    // spends a look.
    assert!(
        !charge(&mut entry, REASON, HEAD_B, 3),
        "one of three looks spent"
    );
    assert_eq!(entry.merge_hold_rechecks, 1);

    // The condition cleared on that pass, so the caller settles the marker the
    // same way it settles `merge_hold` on a merge.
    settle(&mut entry);
    assert!(!entry.merge_hold_cap_escalated);
    assert_eq!(entry.merge_hold_rechecks, 0);
    assert!(!due(&entry, 3), "a settled entry is not reconsidered again");
}

/// The bug this task exists to fix: a hold reason and head that never change
/// must not spend the budget at all, no matter how many passes re-read them —
/// an operator who fixes the condition an hour later still reaches the gate on
/// the very next pass.
#[test]
fn an_unchanged_condition_never_consumes_the_budget() {
    let mut entry = entry();
    entry.phase = LedgerPhase::Escalated;
    escalated(&mut entry, REASON, HEAD_A);

    for _ in 0..50 {
        assert!(due(&entry, 3));
        assert!(!charge(&mut entry, REASON, HEAD_A, 3));
    }
    assert_eq!(entry.merge_hold_rechecks, 0);
    assert!(due(&entry, 3), "still due — nothing has been spent");
}

/// A condition that keeps changing (a worker keeps pushing new failing
/// revisions) still spends one look per genuine change and still converges on
/// exhaustion, so it cannot loop forever either.
#[test]
fn a_condition_that_keeps_changing_still_converges_on_exhaustion() {
    let mut entry = entry();
    entry.phase = LedgerPhase::Escalated;
    escalated(&mut entry, REASON, HEAD_A);

    let heads = ["h1", "h2", "h3"];
    for (i, head) in heads.iter().enumerate() {
        assert!(due(&entry, 3));
        let spent = charge(&mut entry, REASON, head, 3);
        assert_eq!(entry.merge_hold_rechecks, (i + 1) as u32);
        assert_eq!(
            spent,
            i == heads.len() - 1,
            "only the last charge reports spent"
        );
    }
    // The budget is spent: no per-poll loop forever, even though the condition
    // never stopped changing.
    for _ in 0..5 {
        assert!(!due(&entry, 3));
    }
    assert_eq!(entry.merge_hold_rechecks, 3);
}

/// A changed reason on the same head is just as much a genuine re-evaluation
/// as a changed head on the same reason.
#[test]
fn a_changed_reason_on_the_same_head_spends_a_look() {
    let mut entry = entry();
    entry.phase = LedgerPhase::Escalated;
    escalated(&mut entry, "checks_waiting", HEAD_A);

    assert!(!charge(&mut entry, "merge_state:dirty", HEAD_A, 3));
    assert_eq!(entry.merge_hold_rechecks, 1);

    // And now that pair repeats: no further charge.
    assert!(!charge(&mut entry, "merge_state:dirty", HEAD_A, 3));
    assert_eq!(entry.merge_hold_rechecks, 1);
}

#[test]
fn a_zero_budget_never_reconsiders() {
    let mut entry = entry();
    entry.phase = LedgerPhase::Escalated;
    escalated(&mut entry, REASON, HEAD_A);
    assert!(!due(&entry, 0));
}

/// An escalation this module never marked — a closed pull request — is not
/// this budget's to reconsider.
#[test]
fn an_escalation_the_hold_cap_did_not_raise_is_left_alone() {
    let mut entry = entry();
    entry.phase = LedgerPhase::Escalated;
    assert!(!entry.merge_hold_cap_escalated);
    assert!(!due(&entry, 10));
}

#[test]
fn a_non_escalated_entry_is_never_due() {
    let mut entry = entry();
    escalated(&mut entry, REASON, HEAD_A);
    entry.phase = LedgerPhase::PrOpened;
    assert!(!due(&entry, 10));
}

/// The reason `due` and `charge` are two calls rather than one.
///
/// A pass may look at a reconsidered entry without learning anything new —
/// `due` says whether a look is granted, and only `charge` spends one, on the
/// pass that actually re-read an unchanged condition. A `due` that charged by
/// itself would spend the budget on passes that answered nothing new, leaving
/// nothing to spend on the genuine changes this budget exists for.
#[test]
fn looking_without_charging_leaves_the_budget_whole() {
    let mut entry = entry();
    entry.phase = LedgerPhase::Escalated;
    escalated(&mut entry, REASON, HEAD_A);

    // Ten passes that only look: each one is due, none of them spends (the
    // caller simply never calls `charge`).
    for _ in 0..10 {
        assert!(due(&entry, 3), "a pass that charges nothing stays due");
    }
    assert_eq!(entry.merge_hold_rechecks, 0);

    // And the looks the budget *is* for are still all there, spent by genuine
    // changes.
    let heads = ["h1", "h2", "h3"];
    for (i, head) in heads.iter().enumerate() {
        assert!(due(&entry, 3));
        charge(&mut entry, REASON, head, 3);
        assert_eq!(entry.merge_hold_rechecks, (i + 1) as u32);
    }
    assert!(!due(&entry, 3));
}

/// Mid-budget, the deterministic gate clears the entry for good, so the
/// leftover looks are retired rather than left to be misattributed to
/// whatever this entry does next.
#[test]
fn a_verdict_settles_the_marker_before_the_budget_runs_out() {
    let mut entry = entry();
    entry.phase = LedgerPhase::Escalated;
    escalated(&mut entry, REASON, HEAD_A);
    charge(&mut entry, REASON, HEAD_B, 5);
    assert_eq!(entry.merge_hold_rechecks, 1);

    settle(&mut entry);
    assert_eq!(entry.merge_hold_rechecks, 0);
    assert!(!due(&entry, 5));
}

/// The reason the exhaustion decision is recorded on the charge that spends the
/// last look rather than on every later pass: it says once that nothing will
/// reconsider this entry again, and names what it stopped on.
#[test]
fn the_last_charge_reports_itself_and_names_the_condition() {
    let mut entry = entry();
    entry.phase = LedgerPhase::Escalated;
    escalated(&mut entry, REASON, HEAD_A);

    assert!(!charge(&mut entry, REASON, "h1", 2), "not the last look");
    assert!(
        charge(&mut entry, REASON, "h2", 2),
        "the last look reports spent"
    );
    assert_eq!(
        exhausted("checks_not_green"),
        "merge_hold_recheck_exhausted:checks_not_green"
    );
}

/// An entry a build before this module existed already escalated by the hold
/// cap carries no `merge_hold_cap_escalated` — the field did not exist yet —
/// but `merge_hold::charge` already left `merge_hold.escalated` set, and that
/// is enough to start reconsidering it too, not only entries escalated from
/// here on.
#[test]
fn an_entry_a_prior_build_already_escalated_is_reconsidered_from_merge_hold_alone() {
    let mut entry = entry();
    entry.phase = LedgerPhase::Escalated;
    entry.merge_hold.escalated = true;
    assert!(!entry.merge_hold_cap_escalated);

    assert!(due(&entry, 3));
    charge(&mut entry, REASON, HEAD_A, 3);
    assert_eq!(entry.merge_hold_rechecks, 1);
    // The first charge promotes the entry to the durable flag, since a merge
    // on this very pass would let `merge_hold::cleared` wipe
    // `merge_hold.escalated` before the next pass gets to read it.
    assert!(entry.merge_hold_cap_escalated);

    // And having now recorded a baseline, a repeat of that same pair spends
    // nothing further.
    assert!(!charge(&mut entry, REASON, HEAD_A, 3));
    assert_eq!(entry.merge_hold_rechecks, 1);
}
