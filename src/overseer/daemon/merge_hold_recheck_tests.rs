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
    assert!(due(&entry, 3));
    assert!(!charge(&mut entry, 3), "one of three looks spent");
    assert_eq!(entry.merge_hold_rechecks, 1);

    // The condition cleared on that pass, so the caller settles the marker the
    // same way it settles `merge_hold` on a merge.
    settle(&mut entry);
    assert!(!entry.merge_hold_cap_escalated);
    assert_eq!(entry.merge_hold_rechecks, 0);
    assert!(!due(&entry, 3), "a settled entry is not reconsidered again");
}

#[test]
fn a_condition_that_never_clears_stops_being_reconsidered() {
    let mut entry = entry();
    entry.phase = LedgerPhase::Escalated;
    escalated(&mut entry);

    for expected in [1, 2, 3] {
        assert!(due(&entry, 3));
        let spent = charge(&mut entry, 3);
        assert_eq!(entry.merge_hold_rechecks, expected);
        assert_eq!(spent, expected == 3, "only the last charge reports spent");
    }
    // The budget is spent: no per-poll loop forever.
    for _ in 0..5 {
        assert!(!due(&entry, 3));
    }
    assert_eq!(entry.merge_hold_rechecks, 3);
}

#[test]
fn a_zero_budget_never_reconsiders() {
    let mut entry = entry();
    entry.phase = LedgerPhase::Escalated;
    escalated(&mut entry);
    assert!(!due(&entry, 0));
}

/// An escalation this module never marked — a judge veto, a closed pull
/// request — is not this budget's to reconsider.
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
    escalated(&mut entry);
    entry.phase = LedgerPhase::PrOpened;
    assert!(!due(&entry, 10));
}

/// The reason `due` and `charge` are two calls rather than one.
///
/// A reconsidered entry that clears the deterministic gate and waits on a
/// judgment is looked at every pass, but it is not being *re-checked* — the gate
/// already answered, and what it now waits on arrives on the judge queue's own
/// schedule. A judgment round trip runs one session at a time and on a busy
/// queue outlasts the whole budget, so a `due` that charged by itself would
/// spend the budget on waiting. The entry would then sit in `Escalated` with
/// nothing left to bring it back — the exact failure this module exists to end,
/// reproduced through the judge path instead of the gate path.
#[test]
fn looking_without_charging_leaves_the_budget_whole() {
    let mut entry = entry();
    entry.phase = LedgerPhase::Escalated;
    escalated(&mut entry);

    // Ten passes of waiting on a judgment: each one looks, none of them spends.
    for _ in 0..10 {
        assert!(due(&entry, 3), "a pass that charges nothing stays due");
    }
    assert_eq!(entry.merge_hold_rechecks, 0);

    // And the looks the budget *is* for are still all there.
    for expected in [1, 2, 3] {
        assert!(due(&entry, 3));
        charge(&mut entry, 3);
        assert_eq!(entry.merge_hold_rechecks, expected);
    }
    assert!(!due(&entry, 3));
}

/// Mid-budget, a verdict makes the judge the authority that reconsiders this
/// entry (`has_terminal_merge`), so the leftover looks are retired rather than
/// left to be misattributed to whatever the verdict decides next.
#[test]
fn a_verdict_settles_the_marker_before_the_budget_runs_out() {
    let mut entry = entry();
    entry.phase = LedgerPhase::Escalated;
    escalated(&mut entry);
    charge(&mut entry, 5);
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
    escalated(&mut entry);

    assert!(!charge(&mut entry, 2), "not the last look");
    assert!(charge(&mut entry, 2), "the last look reports spent");
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
    charge(&mut entry, 3);
    assert_eq!(entry.merge_hold_rechecks, 1);
    // The first charge promotes the entry to the durable flag, since
    // a Pending outcome on this very pass would let `merge_hold::cleared` wipe
    // `merge_hold.escalated` before the next pass gets to read it.
    assert!(entry.merge_hold_cap_escalated);
}
