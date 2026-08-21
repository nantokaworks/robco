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
fn a_worker_with_no_registered_session_cannot_be_handed_anything() {
    assert_eq!(live_session("agent", &Registry::default()), None);
}

#[test]
fn the_prompt_reaches_the_session_as_one_submission() {
    // tmux delivers a literal newline as a submit, so a prompt sent as authored
    // would enter the worker's prompt line by line and act on the first alone.
    let prompt = crate::overseer::templates::merge_recovery_prompt(
        "#1",
        "task",
        "https://pr/1",
        "merge_state:dirty",
        None,
    );
    assert!(prompt.contains('\n'));
    let flattened = crate::tmux::single_line(&prompt);
    assert!(!flattened.contains('\n'));
    assert!(!flattened.contains("  "));
    // Flattening must not cost the reason or the rails.
    assert!(flattened.contains("merge_state:dirty"));
    assert!(flattened.contains("Never force push"));
}

#[test]
fn every_recorded_reason_names_the_merge_recovery_step() {
    // The decision log is the only place the whole cycle is visible, so each
    // reason has to be greppable and carry what actually happened.
    assert_eq!(
        dispatched("merge_state:dirty"),
        "merge_recovery_dispatched:merge_state:dirty"
    );
    assert_eq!(
        skipped("missing_session:worker-3"),
        "merge_recovery_skipped:missing_session:worker-3"
    );
    assert_eq!(
        skipped("send_failed:tmux send-keys failed"),
        "merge_recovery_skipped:send_failed:tmux send-keys failed"
    );
    assert_eq!(
        undelivered("merge_state:dirty"),
        "merge_recovery_undelivered:merge_state:dirty"
    );
}

/// The exact bug this module exists to catch: `send` reports success (tmux's
/// `send-keys` exits 0 either way — it types keys into a pane, it does not
/// know whether the receiving program acted on them), but the session never
/// shows it started a turn. The charge `plan` took on the way in must not
/// survive that outcome, and the dedup key must clear so the same head is a
/// candidate again on the next pass instead of being permanently marked as
/// already handled.
#[test]
fn an_unconfirmed_delivery_refunds_the_charge_and_clears_the_dedupe_key() {
    let mut entry = entry();
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-1", "base-1", true, 2),
        RecoveryPlan::Dispatch
    );
    assert_eq!(entry.merge_recovery.charged, 1);
    assert_eq!(entry.merge_recovery.head.as_deref(), Some("sha-1"));
    assert_eq!(entry.merge_recovery.base.as_deref(), Some("base-1"));

    refund(&mut entry);

    assert_eq!(entry.merge_recovery.charged, 0);
    assert_eq!(entry.merge_recovery.head, None);
    assert_eq!(entry.merge_recovery.base, None);
    // Un-charged and un-deduped: the same (head, base) pair is a fresh candidate
    // again.
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-1", "base-1", true, 2),
        RecoveryPlan::Dispatch
    );
    assert_eq!(entry.merge_recovery.charged, 1);
}

#[test]
fn refund_never_underflows_a_charge_that_was_never_taken() {
    let mut entry = entry();
    refund(&mut entry);
    assert_eq!(entry.merge_recovery.charged, 0);
    assert_eq!(entry.merge_recovery.head, None);
    assert_eq!(entry.merge_recovery.base, None);
}

#[test]
fn every_undeliverable_reason_names_the_merge_recovery_step() {
    assert_eq!(
        undeliverable("merge_state:dirty"),
        "merge_recovery_undeliverable:merge_state:dirty"
    );
}

/// The bug #436 exists to fix: `refund` resets `merge_recovery.head`/`base`
/// after every failed confirm, so `plan`'s own dedup key can never bound an
/// undelivered handback — it looks like a fresh candidate on every poll.
/// `undelivered_cap_reached` tracks the same head through a separate field
/// that `refund` never touches, so the retry loop actually ends.
#[test]
fn an_undelivered_handback_escalates_after_the_bound() {
    let mut entry = entry();
    assert!(!undelivered_cap_reached(&mut entry, "sha-1", 2));
    assert_eq!(entry.merge_recovery.undelivered_charged, 1);
    assert_eq!(
        entry.merge_recovery.undelivered_head.as_deref(),
        Some("sha-1")
    );

    assert!(undelivered_cap_reached(&mut entry, "sha-1", 2));
    assert_eq!(entry.merge_recovery.undelivered_charged, 2);
}

#[test]
fn a_new_head_resets_the_undelivered_counter_but_a_repeat_head_keeps_it() {
    let mut entry = entry();
    assert!(!undelivered_cap_reached(&mut entry, "sha-1", 3));
    assert!(!undelivered_cap_reached(&mut entry, "sha-1", 3));
    assert_eq!(entry.merge_recovery.undelivered_charged, 2);

    // A worker that pushed a fix presents a new head — a genuinely new
    // handback, not a continuation of the one that never reached the worker.
    assert!(!undelivered_cap_reached(&mut entry, "sha-2", 3));
    assert_eq!(entry.merge_recovery.undelivered_charged, 1);
    assert_eq!(
        entry.merge_recovery.undelivered_head.as_deref(),
        Some("sha-2")
    );
}

#[test]
fn a_zero_undelivered_bound_escalates_on_the_first_attempt() {
    let mut entry = entry();
    assert!(undelivered_cap_reached(&mut entry, "sha-1", 0));
    assert_eq!(entry.merge_recovery.undelivered_charged, 1);
}

#[test]
fn a_confirmed_delivery_clears_the_undelivered_bound() {
    let mut entry = entry();
    assert!(!undelivered_cap_reached(&mut entry, "sha-1", 3));
    clear_undelivered(&mut entry);
    assert_eq!(entry.merge_recovery.undelivered_charged, 0);
    assert_eq!(entry.merge_recovery.undelivered_head, None);
}

/// A pending handback whose entry left `PrOpened` — merged, or escalated
/// through an unrelated path — is discarded rather than delivered on a later
/// pass: the reason it was queued for no longer holds.
#[test]
fn discard_pending_forgets_a_withheld_handback_the_entry_no_longer_needs() {
    let mut entry = entry();
    entry.merge_recovery.head = Some("sha-1".into());
    entry.merge_recovery.base = Some("base-1".into());
    let _ = pending::withhold(&mut entry, "checks_not_green", 5);
    assert!(entry.merge_recovery.pending.is_some());

    discard_pending(&mut entry);
    assert!(entry.merge_recovery.pending.is_none());
}

// `dispatch`'s own busy-session behaviour — withholding instead of sending,
// and escalating once the retry bound is spent — is covered by
// `merge_recovery_dispatch_tests.rs`, split out to keep this file under this
// project's source file size limit.
