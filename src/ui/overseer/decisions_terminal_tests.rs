//! `terminal_reason` on its own (dropr:524): which phase answers, which
//! decision it reads, and what it says when the log has nothing left. Its own
//! file because `decisions_tests.rs` is already at the source-size limit, the
//! same seam `decisions_standoffs_tests.rs` was split on.

use chrono::Utc;

use super::*;
use crate::overseer::ledger::LedgerEntry;

const TASK: &str = "#524";

fn decision(kind: DecisionKind, reason: &str) -> DecisionEntry {
    let mut entry = DecisionEntry::new(kind, reason);
    entry.task = Some(TASK.into());
    entry
}

fn mirror(kind: DecisionKind, reason: &str) -> DecisionEntry {
    let mut entry = decision(kind, reason);
    entry.source = Some(PHASE_MIRROR_SOURCE.into());
    entry
}

fn ledger(phase: LedgerPhase) -> Ledger {
    let mut ledger = Ledger::default();
    ledger.entries.push(LedgerEntry {
        task_id: TASK.into(),
        dropr_task_id: None,
        display_id: TASK.into(),
        repo: "nantokaworks/robco".into(),
        agent_id: "worker-1".into(),
        branch: "robco/worker-1".into(),
        phase,
        dispatched_at: Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: None,
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
    });
    ledger
}

#[test]
fn a_failed_entry_reads_the_reason_its_fail_pass_logged() {
    let decisions = [
        decision(DecisionKind::Hold, "checks_waiting"),
        decision(DecisionKind::Hold, "worker exceeded stuck timeout"),
    ];
    assert_eq!(
        terminal_reason(&ledger(LedgerPhase::Failed), &decisions, "worker-1"),
        Some("worker exceeded stuck timeout".into())
    );
}

#[test]
fn an_escalated_entry_reads_its_escalate_decision() {
    // The `Hold` a triage judge logs when it resolves the exception itself
    // clears `blocked_reason`, but not this: the entry still ended where it
    // ended, and a terminal phase never reverts.
    let decisions = [
        decision(DecisionKind::Escalate, "worker blocked"),
        decision(DecisionKind::Hold, "triage answered the worker"),
    ];
    assert_eq!(
        terminal_reason(&ledger(LedgerPhase::Escalated), &decisions, "worker-1"),
        Some("worker blocked".into())
    );
}

#[test]
fn the_phase_mirror_never_displaces_the_real_reason() {
    let decisions = [
        decision(
            DecisionKind::Escalate,
            "worker released its dropr task lock",
        ),
        mirror(DecisionKind::Escalate, "task_escalated"),
    ];
    assert_eq!(
        terminal_reason(&ledger(LedgerPhase::Escalated), &decisions, "worker-1"),
        Some("worker released its dropr task lock".into())
    );
}

#[test]
fn a_code_shaped_reason_comes_back_as_a_sentence() {
    let decisions = [decision(DecisionKind::Escalate, "merge_hold_stuck:dirty")];
    assert_eq!(
        terminal_reason(&ledger(LedgerPhase::Escalated), &decisions, "worker-1"),
        Some("The pull request has been stuck on the same hold for hours.".into())
    );
}

#[test]
fn an_entry_with_nothing_logged_names_its_own_phase() {
    assert_eq!(
        terminal_reason(&ledger(LedgerPhase::Failed), &[], "worker-1"),
        Some("The worker failed to finish this task.".into())
    );
    assert_eq!(
        terminal_reason(&ledger(LedgerPhase::Escalated), &[], "worker-1"),
        Some("This task needs an operator's decision.".into())
    );
}

#[test]
fn a_blank_reason_falls_back_rather_than_drawing_nothing() {
    let decisions = [decision(DecisionKind::Hold, "   ")];
    assert_eq!(
        terminal_reason(&ledger(LedgerPhase::Failed), &decisions, "worker-1"),
        Some("The worker failed to finish this task.".into())
    );
}

#[test]
fn another_tasks_decision_is_not_borrowed() {
    let mut other = decision(DecisionKind::Hold, "someone else's problem");
    other.task = Some("#999".into());
    assert_eq!(
        terminal_reason(&ledger(LedgerPhase::Failed), &[other], "worker-1"),
        Some("The worker failed to finish this task.".into())
    );
}

#[test]
fn a_phase_the_operator_asked_for_says_nothing() {
    for phase in [
        LedgerPhase::Dispatched,
        LedgerPhase::Claimed,
        LedgerPhase::Working,
        LedgerPhase::PrOpened,
        LedgerPhase::Merged,
    ] {
        assert_eq!(
            terminal_reason(&ledger(phase), &[], "worker-1"),
            None,
            "{phase:?}"
        );
    }
}

#[test]
fn an_agent_with_no_ledger_entry_says_nothing() {
    assert_eq!(
        terminal_reason(&ledger(LedgerPhase::Failed), &[], "worker-2"),
        None
    );
}
