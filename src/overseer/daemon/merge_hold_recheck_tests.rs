use super::*;

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
        merge_hold_cap_escalated: false,
        merge_hold_rechecks: 0,
    }
}

/// The full shape this module exists for: the hold cap escalates the entry, the
/// gate is given a bounded number of fresh looks at it, and once the condition
/// actually clears the caller settles the marker on the merge.
#[test]
fn cap_reached_then_condition_cleared_then_merged() {
    let mut entry = entry();
    entry.phase = LedgerPhase::Escalated;

    escalated(&mut entry);
    assert!(entry.merge_hold_cap_escalated);
    assert_eq!(entry.merge_hold_rechecks, 0);

    // The very next pass is given a fresh look — no operator hand-merge needed,
    // and no waiting for a new head sha.
    assert!(due(&mut entry, 3));
    assert_eq!(entry.merge_hold_rechecks, 1);

    // The condition cleared on that pass, so the caller settles the marker the
    // same way it settles `merge_hold` on a merge.
    settle(&mut entry);
    assert!(!entry.merge_hold_cap_escalated);
    assert_eq!(entry.merge_hold_rechecks, 0);
    assert!(
        !due(&mut entry, 3),
        "a settled entry is not reconsidered again"
    );
}

#[test]
fn a_condition_that_never_clears_stops_being_reconsidered() {
    let mut entry = entry();
    entry.phase = LedgerPhase::Escalated;
    escalated(&mut entry);

    for expected in [1, 2, 3] {
        assert!(due(&mut entry, 3));
        assert_eq!(entry.merge_hold_rechecks, expected);
    }
    // The budget is spent: no per-poll loop forever.
    for _ in 0..5 {
        assert!(!due(&mut entry, 3));
    }
    assert_eq!(entry.merge_hold_rechecks, 3);
}

#[test]
fn a_zero_budget_never_reconsiders() {
    let mut entry = entry();
    entry.phase = LedgerPhase::Escalated;
    escalated(&mut entry);
    assert!(!due(&mut entry, 0));
}

/// An escalation this module never marked — a judge veto, a closed pull
/// request — is not this budget's to reconsider.
#[test]
fn an_escalation_the_hold_cap_did_not_raise_is_left_alone() {
    let mut entry = entry();
    entry.phase = LedgerPhase::Escalated;
    assert!(!entry.merge_hold_cap_escalated);
    assert!(!due(&mut entry, 10));
}

#[test]
fn a_non_escalated_entry_is_never_due() {
    let mut entry = entry();
    escalated(&mut entry);
    entry.phase = LedgerPhase::PrOpened;
    assert!(!due(&mut entry, 10));
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

    assert!(due(&mut entry, 3));
    assert_eq!(entry.merge_hold_rechecks, 1);
    // The first successful call promotes the entry to the durable flag, since
    // a Pending outcome on this very pass would let `merge_hold::cleared` wipe
    // `merge_hold.escalated` before the next pass gets to read it.
    assert!(entry.merge_hold_cap_escalated);
}
