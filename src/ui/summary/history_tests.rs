use chrono::{TimeZone, Utc};

use super::*;

fn entry(display_id: &str, repo: &str, phase: LedgerPhase, settled_minute: u32) -> LedgerEntry {
    LedgerEntry {
        task_id: format!("task-{display_id}"),
        display_id: display_id.into(),
        repo: repo.into(),
        agent_id: "worker".into(),
        branch: "branch".into(),
        phase,
        dispatched_at: Utc.with_ymd_and_hms(2026, 7, 25, 8, 0, 0).unwrap(),
        settled_at: Some(
            Utc.with_ymd_and_hms(2026, 7, 25, 9, settled_minute, 0)
                .unwrap(),
        ),
        retries: 0,
        pr_url: Some("https://github.com/nantokaworks/robco/pull/199".into()),
        branch_updates: 0,
        merge_judge_primes: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        manual_merge_skip: None,
        merge_judge_fail_safes: 0,
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
    }
}

fn ledger(entries: Vec<LedgerEntry>) -> Ledger {
    Ledger {
        entries,
        ..Ledger::default()
    }
}

fn rendered(ledger: &Ledger) -> Vec<String> {
    history_section(ledger, Path::new("/repo"), 8, Locale::En)
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect()
}

#[test]
fn a_settled_entry_is_listed_with_its_phase_time_and_pull_request() {
    let lines = rendered(&ledger(vec![entry(
        "#263",
        "/repo",
        LedgerPhase::Merged,
        30,
    )]));

    assert!(lines.iter().any(|line| line == "HISTORY"));
    assert!(
        lines
            .iter()
            .any(|line| line == "07-25 09:30  #263  merged  PR #199")
    );
}

/// The defect this section exists for: a repository Overseer has never run in
/// used to be indistinguishable from one whose block was simply omitted.
#[test]
fn a_repo_with_no_settled_entries_says_so_rather_than_rendering_nothing() {
    let lines = rendered(&ledger(Vec::new()));

    assert!(lines.iter().any(|line| line == "HISTORY"));
    assert!(
        lines
            .iter()
            .any(|line| line == "overseer has settled no tasks in this repo")
    );
}

/// The ledger holds every repository at once, so a row that belongs to another
/// checkout must never appear under this one.
#[test]
fn entries_from_other_repositories_are_left_out() {
    let lines = rendered(&ledger(vec![
        entry("#100", "/other", LedgerPhase::Merged, 10),
        entry("#263", "/repo", LedgerPhase::Merged, 20),
    ]));

    assert!(lines.iter().any(|line| line.contains("#263")));
    assert!(!lines.iter().any(|line| line.contains("#100")));
}

/// A repository name match would list a second checkout's work here; the ledger
/// records the absolute path dispatch used, so that is what is compared.
#[test]
fn a_repository_of_the_same_name_elsewhere_is_not_this_one() {
    let lines = rendered(&ledger(vec![entry(
        "#100",
        "/elsewhere/repo",
        LedgerPhase::Merged,
        10,
    )]));

    assert!(
        lines
            .iter()
            .any(|line| line == "overseer has settled no tasks in this repo")
    );
}

/// Live entries belong to the OVERSEER frame; the history is what has finished.
#[test]
fn live_entries_are_not_history() {
    let lines = rendered(&ledger(vec![
        entry("#264", "/repo", LedgerPhase::Working, 10),
        entry("#265", "/repo", LedgerPhase::PrOpened, 20),
    ]));

    assert!(
        lines
            .iter()
            .any(|line| line == "overseer has settled no tasks in this repo")
    );
}

#[test]
fn failed_and_escalated_entries_are_history_too() {
    let lines = rendered(&ledger(vec![
        entry("#1", "/repo", LedgerPhase::Failed, 10),
        entry("#2", "/repo", LedgerPhase::Escalated, 20),
    ]));

    assert!(lines.iter().any(|line| line.contains("#1  failed")));
    assert!(lines.iter().any(|line| line.contains("#2  escalated")));
}

#[test]
fn the_newest_settled_entry_is_listed_first() {
    let lines = rendered(&ledger(vec![
        entry("#older", "/repo", LedgerPhase::Merged, 10),
        entry("#newer", "/repo", LedgerPhase::Merged, 50),
    ]));

    let newer = lines.iter().position(|line| line.contains("#newer"));
    let older = lines.iter().position(|line| line.contains("#older"));
    assert!(newer < older);
}

/// An entry that settled before the timestamp existed still has to sort — it
/// falls back to when it was dispatched rather than to the bottom of the list.
#[test]
fn an_entry_without_a_settled_timestamp_falls_back_to_its_dispatch() {
    let mut legacy = entry("#legacy", "/repo", LedgerPhase::Merged, 0);
    legacy.settled_at = None;
    legacy.dispatched_at = Utc.with_ymd_and_hms(2026, 7, 25, 10, 0, 0).unwrap();
    let lines = rendered(&ledger(vec![
        entry("#settled", "/repo", LedgerPhase::Merged, 30),
        legacy,
    ]));

    assert!(lines.iter().any(|line| line.contains("07-25 10:00")));
    let legacy_row = lines.iter().position(|line| line.contains("#legacy"));
    let settled_row = lines.iter().position(|line| line.contains("#settled"));
    assert!(legacy_row < settled_row);
}

/// The ledger is unbounded, so the block caps what it lists — and says how much
/// it left out rather than reading as the whole history.
#[test]
fn a_capped_list_counts_what_it_left_out() {
    let entries = (0..HISTORY_DISPLAY_LIMIT + 3)
        .map(|index| {
            entry(
                &format!("#{index}"),
                "/repo",
                LedgerPhase::Merged,
                index as u32,
            )
        })
        .collect();
    let lines = rendered(&ledger(entries));

    assert_eq!(
        lines.iter().filter(|line| line.contains("merged")).count(),
        HISTORY_DISPLAY_LIMIT
    );
    assert!(lines.iter().any(|line| line == "… and 3 more"));
}

#[test]
fn a_complete_list_carries_no_notice() {
    let entries = (0..HISTORY_DISPLAY_LIMIT)
        .map(|index| {
            entry(
                &format!("#{index}"),
                "/repo",
                LedgerPhase::Merged,
                index as u32,
            )
        })
        .collect();
    let lines = rendered(&ledger(entries));

    assert!(!lines.iter().any(|line| line.contains("more")));
}

/// A pull request URL that does not end in a number is shown intact rather than
/// reported under a number read out of the wrong path segment.
#[test]
fn an_unexpected_pull_request_url_is_shown_as_it_is() {
    let mut odd = entry("#263", "/repo", LedgerPhase::Merged, 30);
    odd.pr_url = Some("https://github.test/pull/199/files".into());
    let lines = rendered(&ledger(vec![odd]));

    assert!(
        lines
            .iter()
            .any(|line| line.ends_with("https://github.test/pull/199/files"))
    );
}

/// An entry can settle without ever opening a pull request — a failed worker,
/// or an escalation before the branch was pushed.
#[test]
fn an_entry_without_a_pull_request_renders_without_one() {
    let mut unopened = entry("#263", "/repo", LedgerPhase::Failed, 30);
    unopened.pr_url = None;
    let lines = rendered(&ledger(vec![unopened]));

    assert!(lines.iter().any(|line| line == "07-25 09:30  #263  failed"));
}
