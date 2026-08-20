//! `reconcile()`-level coverage for lifting an escalation — the counterpart to
//! `apply_resolution_tests.rs`, which pins the pure per-entry logic. This file
//! covers what a whole pass does with it: the same-pass cascade an explicit
//! `unblocked` report gets, and the fresh-clock guarantee a re-escalation
//! depends on.

use chrono::TimeZone;

use super::tests::ledger;
use super::{Action, LedgerPhase, Observations, reconcile};

fn at(minute: u32) -> chrono::DateTime<chrono::Utc> {
    chrono::Utc
        .with_ymd_and_hms(2026, 7, 16, 0, minute, 0)
        .unwrap()
}

/// The daemon-observed path: two passes apart, so `settle` sees the
/// re-escalation the ordinary way, through its normal `original`-phase-free
/// "terminal with no settled_at yet" check.
#[test]
fn a_resolved_entry_that_re_escalates_on_a_later_pass_gets_a_fresh_clock() {
    let mut escalated = ledger();
    escalated.entries[0].phase = LedgerPhase::Escalated;
    escalated.entries[0].settled_at = Some(at(5));
    escalated.entries[0].worker_escalated = true;

    let resolving: Observations = serde_json::from_str(
        r#"{"sessions":[{"agent_id":"worker-1","status":"running","last_activity_at":"2026-07-16T00:06:00Z"}]}"#,
    )
    .unwrap();
    let (resolved, actions) = reconcile(&escalated, &resolving, at(6), 30, 72);
    assert_eq!(resolved.entries[0].phase, LedgerPhase::Working);
    assert_eq!(resolved.entries[0].settled_at, None);
    assert!(actions.iter().any(|action| matches!(
        action,
        Action::LogDecision { message, .. } if message == "resolved_externally:tmux_activity"
    )));

    let released: Observations =
        serde_json::from_str(r#"{"tasks":[{"task_id":"task-131","state":"open"}]}"#).unwrap();
    let (again, _) = reconcile(&resolved, &released, at(7), 30, 72);
    assert_eq!(again.entries[0].phase, LedgerPhase::Escalated);
    assert_eq!(again.entries[0].settled_at, Some(at(7)));
}

/// The explicit-report path resolves inside `apply_inbox`, ahead of `apply_pr`
/// / `apply_task_failure` / `apply_session` in the very same pass — so an
/// entry whose pull request is already open does not wait for the next poll
/// to be recognized as `PrOpened` again.
#[test]
fn an_unblocked_report_cascades_to_the_real_phase_in_the_same_pass() {
    let mut escalated = ledger();
    escalated.entries[0].phase = LedgerPhase::Escalated;
    escalated.entries[0].settled_at = Some(at(5));
    escalated.entries[0].worker_escalated = true;
    escalated.entries[0].pr_url = Some("https://github.test/pull/1".into());

    let observations: Observations = serde_json::from_str(
        r#"{
            "inbox":[{"at":"2026-07-16T00:06:00Z","agent_id":"worker-1","kind":"unblocked"}],
            "prs":[{"taskId":"task-131","url":"https://github.test/pull/1","state":"OPEN","statusCheckRollup":[]}]
        }"#,
    )
    .unwrap();
    let (result, actions) = reconcile(&escalated, &observations, at(6), 30, 72);

    assert_eq!(result.entries[0].phase, LedgerPhase::PrOpened);
    assert!(actions.iter().any(|action| matches!(
        action,
        Action::LogDecision { message, .. } if message == "resolved_externally:explicit_report"
    )));
}

/// The bug the reworked `settle` exists to close: an `unblocked` report can
/// resolve an entry and a same-pass `apply_task_failure` can re-escalate it
/// before the pass ends. The re-escalation must still get its own fresh
/// `settled_at` rather than inheriting the `None` the resolution left behind
/// — otherwise the entry could never be probed for resolution again.
#[test]
fn a_same_pass_resolve_then_re_escalate_still_gets_a_fresh_clock() {
    let mut escalated = ledger();
    escalated.entries[0].phase = LedgerPhase::Escalated;
    escalated.entries[0].settled_at = Some(at(5));
    escalated.entries[0].worker_escalated = true;

    let observations: Observations = serde_json::from_str(
        r#"{
            "inbox":[{"at":"2026-07-16T00:06:00Z","agent_id":"worker-1","kind":"unblocked"}],
            "tasks":[{"task_id":"task-131","state":"open"}]
        }"#,
    )
    .unwrap();
    let (result, _) = reconcile(&escalated, &observations, at(6), 30, 72);

    assert_eq!(result.entries[0].phase, LedgerPhase::Escalated);
    assert_eq!(result.entries[0].settled_at, Some(at(6)));
}

/// The merge-escalation regression at the `reconcile()` level: an entry
/// escalated by the merge subsystem (`worker_escalated: false`, e.g. a
/// spent hold budget or an exhausted recovery budget) must not be
/// resolved by either path — not the
/// daemon's own observed activity, and not a worker's `unblocked` report,
/// since the worker's worktree and session stay alive through a merge-gate
/// escalation too and it may not know the block is not its own.
#[test]
fn a_merge_gate_escalation_survives_both_resolution_paths() {
    let mut escalated = ledger();
    escalated.entries[0].phase = LedgerPhase::Escalated;
    escalated.entries[0].settled_at = Some(at(5));
    escalated.entries[0].worker_escalated = false;
    escalated.entries[0].merge_hold_cap_escalated = true;

    let observations: Observations = serde_json::from_str(
        r#"{
            "inbox":[{"at":"2026-07-16T00:06:00Z","agent_id":"worker-1","kind":"unblocked"}],
            "sessions":[{"agent_id":"worker-1","status":"running","last_activity_at":"2026-07-16T00:06:00Z"}],
            "tasks":[{"task_id":"task-131","state":"in_progress","updated_at":"2026-07-16T00:06:00Z"}]
        }"#,
    )
    .unwrap();
    let (result, actions) = reconcile(&escalated, &observations, at(6), 30, 72);

    assert_eq!(result.entries[0].phase, LedgerPhase::Escalated);
    assert_eq!(result.entries[0].settled_at, Some(at(5)));
    assert!(result.entries[0].merge_hold_cap_escalated);
    assert!(!actions.iter().any(|action| matches!(
        action,
        Action::LogDecision { message, .. } if message.starts_with("resolved_externally:")
    )));
}
