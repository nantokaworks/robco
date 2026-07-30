use std::cell::RefCell;

use chrono::Utc;

use super::*;
use crate::overseer::ledger::LedgerPhase;

fn candidate(id: &str) -> Candidate {
    Candidate {
        task_id: id.into(),
        display_id: format!("#{id}"),
        title: "title".into(),
        repo: "repo".into(),
        author: "author".into(),
        priority: "medium".into(),
        workspace: "workspace".into(),
        priority_score: None,
        status: "open".into(),
    }
}

fn entry(task: &str, phase: LedgerPhase) -> crate::overseer::ledger::LedgerEntry {
    crate::overseer::ledger::LedgerEntry {
        task_id: task.into(),
        display_id: format!("#{task}"),
        repo: "repo".into(),
        agent_id: format!("agent-{task}"),
        branch: task.into(),
        phase,
        dispatched_at: Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: None,
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        manual_merge_skip: None,
        merge_judge_fail_safes: 0,
        merge_hold_cap_escalated: false,
        merge_hold_rechecks: 0,
        merge_hold_recheck_reason: None,
        merge_hold_recheck_head: None,
        merge_hold_stuck_notified: false,
    }
}

fn ledger(entries: Vec<crate::overseer::ledger::LedgerEntry>) -> Ledger {
    Ledger {
        entries,
        ..Ledger::default()
    }
}

struct Recorder(RefCell<Vec<DecisionEntry>>);

impl Recorder {
    fn new() -> Self {
        Self(RefCell::new(Vec::new()))
    }

    fn append(&self) -> impl FnMut(&DecisionEntry) -> Result<()> + '_ {
        |entry: &DecisionEntry| {
            self.0.borrow_mut().push(entry.clone());
            Ok(())
        }
    }

    fn reasons(&self) -> Vec<String> {
        self.0.borrow().iter().map(|e| e.reason.clone()).collect()
    }
}

#[test]
fn is_drained_requires_both_no_candidates_and_no_live_ledger_entries() {
    assert!(is_drained(&[], &ledger(vec![])));
    assert!(!is_drained(&[candidate("1")], &ledger(vec![])));
    assert!(!is_drained(
        &[],
        &ledger(vec![entry("1", LedgerPhase::Working)])
    ));
    assert!(is_drained(
        &[],
        &ledger(vec![entry("1", LedgerPhase::Merged)])
    ));
}

#[test]
fn a_daemon_started_against_an_already_empty_board_does_not_announce_a_drain() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("queue_drained.json");
    let recorder = Recorder::new();

    check_at(&path, &[], &ledger(vec![]), recorder.append()).unwrap();

    assert!(recorder.reasons().is_empty());
}

#[test]
fn a_transition_from_busy_to_drained_fires_once_and_repeated_drained_passes_stay_silent() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("queue_drained.json");
    let recorder = Recorder::new();

    // First pass: board is busy (a candidate is waiting). No announcement.
    check_at(&path, &[candidate("1")], &ledger(vec![]), recorder.append()).unwrap();
    assert!(recorder.reasons().is_empty());

    // Second pass: the board is drained now. Exactly one announcement.
    check_at(&path, &[], &ledger(vec![]), recorder.append()).unwrap();
    assert_eq!(recorder.reasons(), ["queue_drained"]);

    // Several more consecutive drained passes: no repeats.
    for _ in 0..3 {
        check_at(&path, &[], &ledger(vec![]), recorder.append()).unwrap();
    }
    assert_eq!(recorder.reasons(), ["queue_drained"]);
}

#[test]
fn a_restart_against_an_already_drained_board_does_not_re_announce() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("queue_drained.json");

    // First "process": drains once and persists the edge.
    let first = Recorder::new();
    check_at(&path, &[candidate("1")], &ledger(vec![]), first.append()).unwrap();
    check_at(&path, &[], &ledger(vec![]), first.append()).unwrap();
    assert_eq!(first.reasons(), ["queue_drained"]);

    // A fresh "process" (new in-memory state, same persisted file) observes
    // the same already-drained board and must not re-announce.
    let second = Recorder::new();
    check_at(&path, &[], &ledger(vec![]), second.append()).unwrap();
    assert!(second.reasons().is_empty());
}

#[test]
fn drain_is_suppressed_while_a_non_terminal_ledger_entry_is_still_live() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("queue_drained.json");
    let recorder = Recorder::new();

    check_at(&path, &[candidate("1")], &ledger(vec![]), recorder.append()).unwrap();
    // Candidates cleared, but a worker is still working — not drained.
    check_at(
        &path,
        &[],
        &ledger(vec![entry("1", LedgerPhase::Working)]),
        recorder.append(),
    )
    .unwrap();
    assert!(recorder.reasons().is_empty());
}
