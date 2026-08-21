use super::*;
use crate::overseer::ledger::LedgerPhase;
use crate::overseer::monitor::Observations;
use chrono::{DateTime, Duration, TimeZone, Utc};

/// The fixture day's fixed `now`, matched to every `entry()`'s `dispatched_at`
/// so `worth_probing`'s thirty-day settled-age window never trims a fixture
/// out from under `beyond_reach` by accident.
fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 20, 0, 0, 0).unwrap()
}

/// An entry dispatched `minute` minutes into the fixture day, so recency is
/// stated by the test rather than inferred from vector order.
fn entry(task: &str, repo: &str, phase: LedgerPhase, minute: i64) -> LedgerEntry {
    LedgerEntry {
        task_id: task.into(),
        dropr_task_id: None,
        display_id: format!("#{task}"),
        repo: repo.into(),
        agent_id: format!("worker-{task}"),
        branch: task.into(),
        phase,
        dispatched_at: Utc.with_ymd_and_hms(2026, 7, 20, 0, 0, 0).unwrap()
            + Duration::minutes(minute),
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
    let dropped = prune(&mut board, &[], &Observations::default(), now(), 2);
    assert_eq!(tasks(&board), ["2", "3"]);
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0].0.task_id, "1");
}

#[test]
fn counts_the_window_per_repository() {
    let mut board = ledger(vec![
        entry("1", "/a", LedgerPhase::Merged, 0),
        entry("2", "/a", LedgerPhase::Merged, 10),
        entry("3", "/b", LedgerPhase::Merged, 20),
    ]);
    prune(&mut board, &[], &Observations::default(), now(), 1);
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
    assert!(prune(&mut board, &[], &Observations::default(), now(), 1).is_empty());
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
    let dropped = prune(
        &mut board,
        &["worker-1".to_string()],
        &Observations::default(),
        now(),
        1,
    );
    assert_eq!(tasks(&board), ["1", "3"]);
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0].0.task_id, "2");
}

#[test]
fn a_zero_window_keeps_every_terminal_entry() {
    let mut board = ledger(vec![
        entry("1", "/repo", LedgerPhase::Merged, 0),
        entry("2", "/repo", LedgerPhase::Merged, 10),
    ]);
    assert!(prune(&mut board, &[], &Observations::default(), now(), 0).is_empty());
    assert_eq!(tasks(&board), ["1", "2"]);
}

#[test]
fn a_ledger_inside_its_window_is_left_alone() {
    let mut board = ledger(vec![
        entry("1", "/repo", LedgerPhase::Merged, 0),
        entry("2", "/repo", LedgerPhase::Failed, 10),
    ]);
    assert!(prune(&mut board, &[], &Observations::default(), now(), 2).is_empty());
    assert_eq!(tasks(&board), ["1", "2"]);
}

#[test]
fn entries_sharing_a_dispatch_time_rank_by_ledger_order() {
    let mut board = ledger(vec![
        entry("1", "/repo", LedgerPhase::Merged, 0),
        entry("2", "/repo", LedgerPhase::Merged, 0),
    ]);
    prune(&mut board, &[], &Observations::default(), now(), 1);
    assert_eq!(tasks(&board), ["2"]);
}

#[test]
fn sweeps_an_escalated_entry_whose_dropr_task_is_confirmed_absent() {
    let mut board = ledger(vec![entry("1", "/repo", LedgerPhase::Escalated, 0)]);
    let observations = Observations {
        tasks: vec![crate::overseer::monitor::TaskObservation {
            task_id: "1".into(),
            state: TASK_ABSENT.into(),
            updated_at: None,
        }],
        ..Observations::default()
    };
    // Well inside the retention window — the sweep fires regardless.
    let dropped = prune(&mut board, &[], &observations, now(), 100);
    assert!(tasks(&board).is_empty());
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0].0.task_id, "1");
}

#[test]
fn sweeps_an_escalated_entry_whose_pull_request_is_confirmed_gone() {
    let mut board = ledger(vec![entry("1", "/repo", LedgerPhase::Escalated, 0)]);
    board.entries[0].pr_url = Some("https://github.com/nantokaworks/robco/pull/1".into());
    // No matching `PrObservation` and no matching `ObservationError` for
    // task "1": the probe ran and confirmed the pull request is gone.
    let dropped = prune(&mut board, &[], &Observations::default(), now(), 100);
    assert!(tasks(&board).is_empty());
    assert_eq!(dropped.len(), 1);
}

#[test]
fn leaves_an_escalated_entry_alone_when_its_pull_request_probe_only_failed() {
    let mut board = ledger(vec![entry("1", "/repo", LedgerPhase::Escalated, 0)]);
    board.entries[0].pr_url = Some("https://github.com/nantokaworks/robco/pull/1".into());
    let observations = Observations {
        errors: vec![
            crate::overseer::monitor::ObservationError::new("gh PR list exited 1")
                .about("1", "/repo"),
        ],
        ..Observations::default()
    };
    assert!(prune(&mut board, &[], &observations, now(), 100).is_empty());
    assert_eq!(tasks(&board), ["1"]);
}

#[test]
fn leaves_an_escalated_entry_alone_when_it_never_opened_a_pull_request() {
    // No `pr_url`: the worker was killed before it opened one, not a pull
    // request that went away.
    let mut board = ledger(vec![entry("1", "/repo", LedgerPhase::Escalated, 0)]);
    assert!(prune(&mut board, &[], &Observations::default(), now(), 100).is_empty());
    assert_eq!(tasks(&board), ["1"]);
}

#[test]
fn never_sweeps_a_confirmed_gone_entry_whose_worker_is_still_registered() {
    let mut board = ledger(vec![entry("1", "/repo", LedgerPhase::Escalated, 0)]);
    board.entries[0].pr_url = Some("https://github.com/nantokaworks/robco/pull/1".into());
    let dropped = prune(
        &mut board,
        &["worker-1".to_string()],
        &Observations::default(),
        now(),
        100,
    );
    assert!(dropped.is_empty());
    assert_eq!(tasks(&board), ["1"]);
}
