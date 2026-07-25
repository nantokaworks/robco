use super::*;
use crate::overseer::ledger::LedgerPhase;
use chrono::{Duration, TimeZone, Utc};

/// An entry dispatched `minute` minutes into the fixture day, so recency is
/// stated by the test rather than inferred from vector order.
fn entry(task: &str, repo: &str, phase: LedgerPhase, minute: i64) -> LedgerEntry {
    LedgerEntry {
        task_id: task.into(),
        display_id: format!("#{task}"),
        repo: repo.into(),
        agent_id: format!("worker-{task}"),
        branch: task.into(),
        phase,
        dispatched_at: Utc.with_ymd_and_hms(2026, 7, 20, 0, 0, 0).unwrap()
            + Duration::minutes(minute),
        retries: 0,
        pr_url: None,
        branch_updates: 0,
        merge_recovery: Default::default(),
        manual_merge_skip: None,
    }
}

fn ledger(entries: Vec<LedgerEntry>) -> Ledger {
    Ledger {
        entries,
        ..Ledger::default()
    }
}

fn tasks(ledger: &Ledger) -> Vec<&str> {
    ledger
        .entries
        .iter()
        .map(|entry| entry.task_id.as_str())
        .collect()
}

#[test]
fn keeps_the_most_recent_terminal_entries_and_their_ledger_order() {
    let mut board = ledger(vec![
        entry("1", "/repo", LedgerPhase::Merged, 0),
        entry("2", "/repo", LedgerPhase::Failed, 30),
        entry("3", "/repo", LedgerPhase::Merged, 20),
    ]);
    let dropped = prune(&mut board, &[], 2);
    assert_eq!(tasks(&board), ["2", "3"]);
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0].task_id, "1");
}

#[test]
fn counts_the_window_per_repository() {
    let mut board = ledger(vec![
        entry("1", "/a", LedgerPhase::Merged, 0),
        entry("2", "/a", LedgerPhase::Merged, 10),
        entry("3", "/b", LedgerPhase::Merged, 20),
    ]);
    prune(&mut board, &[], 1);
    assert_eq!(tasks(&board), ["2", "3"]);
}

#[test]
fn never_drops_a_live_entry_or_charges_it_to_the_window() {
    let mut board = ledger(vec![
        entry("1", "/repo", LedgerPhase::Working, 0),
        entry("2", "/repo", LedgerPhase::Dispatched, 10),
        entry("3", "/repo", LedgerPhase::Merged, 20),
    ]);
    let before = board.active_workers();
    assert!(prune(&mut board, &[], 1).is_empty());
    assert_eq!(tasks(&board), ["1", "2", "3"]);
    assert_eq!(board.active_workers(), before);
}

#[test]
fn never_drops_a_terminal_entry_whose_worker_is_still_registered() {
    let mut board = ledger(vec![
        entry("1", "/repo", LedgerPhase::Merged, 0),
        entry("2", "/repo", LedgerPhase::Escalated, 10),
        entry("3", "/repo", LedgerPhase::Merged, 20),
    ]);
    let dropped = prune(&mut board, &["worker-1".to_string()], 1);
    assert_eq!(tasks(&board), ["1", "3"]);
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0].task_id, "2");
}

#[test]
fn a_zero_window_keeps_every_terminal_entry() {
    let mut board = ledger(vec![
        entry("1", "/repo", LedgerPhase::Merged, 0),
        entry("2", "/repo", LedgerPhase::Merged, 10),
    ]);
    assert!(prune(&mut board, &[], 0).is_empty());
    assert_eq!(tasks(&board), ["1", "2"]);
}

#[test]
fn a_ledger_inside_its_window_is_left_alone() {
    let mut board = ledger(vec![
        entry("1", "/repo", LedgerPhase::Merged, 0),
        entry("2", "/repo", LedgerPhase::Failed, 10),
    ]);
    assert!(prune(&mut board, &[], 2).is_empty());
    assert_eq!(tasks(&board), ["1", "2"]);
}

#[test]
fn entries_sharing_a_dispatch_time_rank_by_ledger_order() {
    let mut board = ledger(vec![
        entry("1", "/repo", LedgerPhase::Merged, 0),
        entry("2", "/repo", LedgerPhase::Merged, 0),
    ]);
    prune(&mut board, &[], 1);
    assert_eq!(tasks(&board), ["2"]);
}
