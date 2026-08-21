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

#[test]
fn a_dispatch_is_a_retry_of_undelivered_only_when_the_charged_head_matches_it() {
    let mut entry = entry();
    // Nothing charged yet, nothing undelivered yet.
    assert!(!is_retry_of_undelivered(&entry));

    // A genuinely new charge, no prior undelivered failure on record.
    entry.merge_recovery.head = Some("sha-1".into());
    assert!(!is_retry_of_undelivered(&entry));

    // The charged head matches the head a previous confirm failed against —
    // this is the exact case `dispatch` must not blindly retype into.
    entry.merge_recovery.undelivered_head = Some("sha-1".into());
    assert!(is_retry_of_undelivered(&entry));

    // A worker that pushed presents a new head: no longer a retry of the old
    // failure, even though an undelivered record still exists for it.
    entry.merge_recovery.head = Some("sha-2".into());
    assert!(!is_retry_of_undelivered(&entry));
}

/// The exact bug #457 exists to stop: a retry for a (head, base) pair whose
/// previous attempt could not be confirmed must not retype the prompt into a
/// session that already looks busy — the session may already be running that
/// earlier, genuinely-delivered attempt, and typing over it would duplicate
/// the handback instead of confirming it.
#[test]
fn a_busy_retry_of_an_unconfirmed_delivery_is_refunded_without_resending() {
    let session = format!("robco-test-dispatch-busy-{}", std::process::id());
    if crate::tmux::new_session(&session, &std::env::temp_dir(), "sh", &[]).is_err() {
        // No usable tmux in this environment — the pure-logic tests above
        // already cover the branches this test would exercise.
        return;
    }
    let _ = crate::tmux::send_literal_text(&session, "esc to interrupt");

    let mut entry = entry();
    // The state right after `plan` charged this poll's attempt for a head
    // that a previous attempt already failed to confirm.
    entry.merge_recovery.charged = 1;
    entry.merge_recovery.head = Some("sha-1".into());
    entry.merge_recovery.base = Some("base-1".into());
    entry.merge_recovery.undelivered_head = Some("sha-1".into());
    entry.merge_recovery.undelivered_charged = 1;

    let registry = registry_with_session(&session);
    dispatch(&mut entry, "merge_state:dirty", &registry, None, 2).unwrap();

    // Un-charged like any other refund, and the undelivered bound this poll
    // neither confirmed nor disproved is left exactly where it was — a busy
    // read must not spend either budget.
    assert_eq!(entry.merge_recovery.charged, 0);
    assert_eq!(entry.merge_recovery.head, None);
    assert_eq!(entry.merge_recovery.base, None);
    assert_eq!(entry.merge_recovery.undelivered_charged, 1);
    // A hold, not an escalation or a dispatched handback: the entry stays
    // exactly where it started.
    assert_eq!(entry.phase, LedgerPhase::PrOpened);

    let _ = crate::tmux::kill_session(&session);
}

// A first attempt for a head that has never failed confirmation is not a
// retry (see `a_dispatch_is_a_retry_of_undelivered_only_when_the_charged_head_matches_it`
// above), so an incidentally busy session never reaches the short-circuit
// added for #457 — `dispatch` falls through to its normal send/confirm path,
// which is not re-tested here since it calls out to the dropr task API on a
// confirmed delivery.

fn registry_with_session(session: &str) -> Registry {
    let now = chrono::Local::now();
    let agent = crate::model::AgentNode {
        id: "agent".into(),
        parent_agent_id: Some(crate::overseer::OVERSEER_AGENT_ID.into()),
        title: "worker".into(),
        task_number: None,
        worktree_path: "/tmp/agent".into(),
        branch: "branch".into(),
        base_commit: String::new(),
        program: "claude".into(),
        claude_session_id: None,
        profile: None,
        tmux_session: session.into(),
        created_at: now,
        updated_at: now,
        status: crate::model::Status::Running,
        worktree_missing: false,
        merge_error: None,
        last_capture: None,
        last_spinner: None,
        last_change_at: None,
        last_auto_accept_at: None,
        shell_working: false,
        mcp_active: false,
        pane_pid: None,
        tracked_command: None,
        subagents: Vec::new(),
        children: Vec::new(),
    };
    Registry {
        version: 1,
        repos: vec![crate::model::RepoNode {
            path: "/repo".into(),
            name: "repo".into(),
            remote_url: None,
            pinned: false,
            agents: vec![agent],
            dropr: None,
            dropr_tasks: crate::dropr::DroprTaskFetch::default(),
            main_status: None,
            main_last_capture: None,
            main_last_spinner: None,
            main_last_change_at: None,
            main_shell_working: false,
            main_mcp_active: false,
            main_pane_pid: None,
            main_tracked_command: None,
            main_subagents_active: 0,
            main_behind_origin: None,
            checkout_state: None,
        }],
    }
}
