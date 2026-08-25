use super::*;
use crate::overseer::{
    ledger::{Ledger, LedgerPhase},
    logging::{self, DecisionKind},
};
use chrono::Utc;

pub(super) fn case() -> ExceptionCase {
    ExceptionCase {
        id: "case-1".into(),
        kind: "worker_failed".into(),
        task_id: "task-1".into(),
        dropr_task_id: Some("task-1".into()),
        display_id: "#1".into(),
        worker_id: "worker-1".into(),
        repo: "/repo".into(),
        reason: "stuck".into(),
        task_state: "in_progress".into(),
    }
}

pub(super) fn ledger() -> Ledger {
    Ledger {
        entries: vec![LedgerEntry {
            task_id: "task-1".into(),
            dropr_task_id: None,
            display_id: "#1".into(),
            repo: "/repo".into(),
            agent_id: "worker-1".into(),
            branch: "task-1".into(),
            phase: LedgerPhase::Failed,
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
        }],
        ..Ledger::default()
    }
}

pub(super) fn no_scribble(_: &str, _: &str, _: &str) -> crate::dropr::WriteResult {
    Ok(())
}

#[test]
fn skip_result_adds_task_to_skip_list() {
    let temp = tempfile::tempdir().unwrap();
    let mut ledger = ledger();
    apply_session_result_with(
        SessionResult::Result(br#"{"outcome":"skip","reason":"not actionable"}"#.to_vec()),
        &mut ledger,
        &case(),
        &temp.path().join("decisions.jsonl"),
        &no_scribble,
    )
    .unwrap();
    assert_eq!(ledger.skip_list, ["task-1"]);
}

/// Triage escalates outside the reconcile pass, and the next pass sees an entry
/// that is already terminal and leaves it alone — so if triage does not stamp
/// the entry here, nothing ever will and the history cannot order it.
#[test]
fn malformed_result_escalates_and_records_when_the_entry_settled() {
    let temp = tempfile::tempdir().unwrap();
    let mut ledger = ledger();
    let escalate = |ledger: &mut Ledger| {
        apply_session_result_with(
            SessionResult::Result(b"not-json".to_vec()),
            ledger,
            &case(),
            &temp.path().join("decisions.jsonl"),
            &no_scribble,
        )
        .unwrap();
    };

    escalate(&mut ledger);
    assert_eq!(ledger.entries[0].phase, LedgerPhase::Escalated);
    let settled = ledger.entries[0].settled_at;
    assert!(settled.is_some());

    // A repeat escalation of the same entry must not move the timestamp.
    escalate(&mut ledger);
    assert_eq!(ledger.entries[0].settled_at, settled);
}

/// A triage escalation never goes through `merge_hold::charge`, so without
/// this it never earns a reconsideration budget and a green pull request
/// behind it would sit parked forever — see dropr:401.
#[test]
fn escalating_an_entry_with_an_open_pull_request_grants_reconsideration() {
    let temp = tempfile::tempdir().unwrap();
    let mut ledger = ledger();
    ledger.entries[0].pr_url = Some("https://github.com/nantokaworks/robco/pull/1".into());
    apply_session_result_with(
        SessionResult::TimedOut,
        &mut ledger,
        &case(),
        &temp.path().join("decisions.jsonl"),
        &no_scribble,
    )
    .unwrap();
    let entry = &ledger.entries[0];
    assert_eq!(entry.phase, LedgerPhase::Escalated);
    assert!(!entry.worker_escalated);
    assert!(entry.merge_hold_cap_escalated);
    assert_eq!(entry.merge_hold_rechecks, 0);
}

#[test]
fn escalating_an_entry_without_a_pull_request_grants_no_reconsideration() {
    let temp = tempfile::tempdir().unwrap();
    let mut ledger = ledger();
    apply_session_result_with(
        SessionResult::TimedOut,
        &mut ledger,
        &case(),
        &temp.path().join("decisions.jsonl"),
        &no_scribble,
    )
    .unwrap();
    assert!(!ledger.entries[0].merge_hold_cap_escalated);
}

/// The note is the only explanation an operator reading dropr gets for an
/// escalation, so losing it must still be visible in the decision log — but
/// a write robco failed at is robco's own problem, not a second operator
/// decision, so it folds into the one escalation instead of paging a
/// second time (dropr:556).
#[test]
fn an_escalation_note_that_did_not_land_folds_into_the_one_escalation() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("decisions.jsonl");
    let mut ledger = ledger();
    apply_session_result_with(
        SessionResult::TimedOut,
        &mut ledger,
        &case(),
        &log_path,
        &|_, _, _| Err(crate::dropr::WriteError::Refused("Method not found".into())),
    )
    .unwrap();

    let escalations = logging::tail_from(&log_path, 10)
        .unwrap()
        .into_iter()
        .filter(|entry| entry.kind == DecisionKind::Escalate)
        .map(|entry| entry.reason)
        .collect::<Vec<_>>();
    assert_eq!(
        escalations.len(),
        1,
        "a lost note must not page a second time: {escalations:?}"
    );
    assert_eq!(
        escalations[0],
        "triage session timed out (escalation note not recorded in dropr: \
         refused: Method not found)"
    );
}

/// A case whose entry has no known dropr task (`ExceptionCase::dropr_task_id`
/// is `None` — an entry adopted from a live agent, not dispatched through
/// dropr) has nowhere to record the note. The write must not be attempted at
/// all — this is what dropr:531's `boq-hQwQ` refusal was: `case.task_id` was
/// an agent id sent to dropr as if it were a task id — and no failure is
/// logged either, since nothing was lost.
#[test]
fn a_case_with_no_dropr_task_does_not_call_dropr_and_logs_no_failure() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("decisions.jsonl");
    let mut ledger = ledger();
    let case = ExceptionCase {
        dropr_task_id: None,
        ..case()
    };
    apply_session_result_with(
        SessionResult::TimedOut,
        &mut ledger,
        &case,
        &log_path,
        &|task_id, _, _| panic!("dropr must not be called with no known dropr task: {task_id}"),
    )
    .unwrap();

    let escalations = logging::tail_from(&log_path, 10)
        .unwrap()
        .into_iter()
        .filter(|entry| entry.kind == DecisionKind::Escalate)
        .map(|entry| entry.reason)
        .collect::<Vec<_>>();
    assert!(
        !escalations
            .iter()
            .any(|reason| reason.starts_with("escalation note not recorded in dropr:")),
        "a case with no dropr task must not log a lost-note escalation: {escalations:?}"
    );
}

#[test]
fn unknown_action_is_ignored_and_logged_not_escalated() {
    // A schema mismatch on `action` — including a name the enum does not
    // recognise — is a model formatting slip `result::parse` now recovers
    // from in place: the action is dropped, but `outcome`/`reason` parsed
    // fine and are honoured as they are. See dropr:401.
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("decisions.jsonl");
    let mut ledger = ledger();
    let before = ledger.entries[0].phase;
    let raw = br#"{
        "outcome":"resolved",
        "action":{"name":"run_shell","command":"rm -rf /"},
        "reason":"try command"
    }"#;
    apply_session_result_with(
        SessionResult::Result(raw.to_vec()),
        &mut ledger,
        &case(),
        &log_path,
        &no_scribble,
    )
    .unwrap();
    let log = fs::read_to_string(log_path).unwrap();
    assert!(log.contains("triage action ignored"));
    assert!(log.contains("run_shell"));
    assert_eq!(ledger.entries[0].phase, before);
}

// `result::parse`-level tests (schema-mismatch recovery, policy rejection)
// live in `result_tests.rs`, mounted from `result.rs` directly — see
// dropr:401.
