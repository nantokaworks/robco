use super::*;
use crate::overseer::monitor::{InboxObservation, PrObservation};
use chrono::Utc;

fn now() -> DateTime<Utc> {
    "2026-07-28T00:00:00Z".parse().unwrap()
}

fn entry(task: &str, agent: &str, phase: LedgerPhase) -> LedgerEntry {
    LedgerEntry {
        task_id: task.into(),
        display_id: format!("#{task}"),
        repo: "repo".into(),
        agent_id: agent.into(),
        branch: task.into(),
        phase,
        dispatched_at: Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: Some("https://pr".into()),
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        manual_merge_skip: None,
        merge_judge_fail_safes: 0,
        merge_hold_cap_escalated: false,
        merge_hold_rechecks: 0,
        merge_hold_recheck_reason: None,
        merge_hold_recheck_head: None,
    }
}

fn ledger(entries: Vec<LedgerEntry>) -> Ledger {
    Ledger {
        entries,
        ..Ledger::default()
    }
}

fn reasons<'a>(events: &[(&LedgerEntry, DecisionKind, &'a str)]) -> Vec<&'a str> {
    events.iter().map(|(_, _, reason)| *reason).collect()
}

#[test]
fn event_carries_pr_url() {
    let entry = entry("task", "agent", LedgerPhase::PrOpened);
    let mut decision = DecisionEntry::new(DecisionKind::Hold, "pr_opened");
    decision.pr_url.clone_from(&entry.pr_url);
    assert_eq!(decision.pr_url.as_deref(), Some("https://pr"));
}

#[test]
fn a_brand_new_dispatched_entry_fires_task_started_once() {
    let previous = ledger(vec![]);
    let next = ledger(vec![entry("1", "worker-1", LedgerPhase::Dispatched)]);
    let events = transitions(&previous, &next, &Observations::default(), now());
    assert_eq!(reasons(&events), ["task_started"]);
}

#[test]
fn an_unchanged_dispatched_entry_fires_nothing() {
    let board = ledger(vec![entry("1", "worker-1", LedgerPhase::Dispatched)]);
    let events = transitions(&board, &board, &Observations::default(), now());
    assert!(events.is_empty());
}

#[test]
fn each_terminal_transition_fires_its_own_reason_once() {
    for (phase, reason) in [
        (LedgerPhase::Merged, "merged"),
        (LedgerPhase::Failed, "task_failed"),
        (LedgerPhase::Escalated, "task_escalated"),
    ] {
        let previous = ledger(vec![entry("1", "worker-1", LedgerPhase::Working)]);
        let next = ledger(vec![entry("1", "worker-1", phase)]);
        let events = transitions(&previous, &next, &Observations::default(), now());
        assert_eq!(reasons(&events), [reason], "phase {phase:?}");
    }
}

#[test]
fn merged_does_not_also_produce_a_finished_event() {
    let previous = ledger(vec![entry("1", "worker-1", LedgerPhase::PrOpened)]);
    let next = ledger(vec![entry("1", "worker-1", LedgerPhase::Merged)]);
    let events = transitions(&previous, &next, &Observations::default(), now());
    assert_eq!(reasons(&events), ["merged"]);
}

#[test]
fn a_blocked_inbox_report_fires_worker_blocked_for_its_agent() {
    let board = ledger(vec![entry("1", "worker-1", LedgerPhase::Working)]);
    let observed = Observations {
        inbox: vec![InboxObservation {
            at: Utc::now(),
            agent_id: "worker-1".into(),
            kind: "blocked".into(),
        }],
        ..Observations::default()
    };
    let events = transitions(&board, &board, &observed, now());
    assert_eq!(reasons(&events), ["worker_blocked"]);
}

fn merged_pr_observation(task: &str, merged_at: DateTime<Utc>) -> Observations {
    Observations {
        prs: vec![PrObservation {
            task_id: Some(task.into()),
            url: Some("https://pr".into()),
            state: "MERGED".into(),
            merged_at: Some(merged_at),
            ..PrObservation::default()
        }],
        ..Observations::default()
    }
}

/// The batch this task exists for: several entries the reconcile pass
/// catches up on at once must not storm Discord with "merged" events for
/// work that finished weeks ago.
#[test]
fn a_retroactive_batch_of_escalated_and_failed_catch_ups_notifies_nothing() {
    let previous = ledger(vec![
        entry("1", "worker-1", LedgerPhase::Escalated),
        entry("2", "worker-2", LedgerPhase::Failed),
    ]);
    let next = ledger(vec![
        entry("1", "worker-1", LedgerPhase::Merged),
        entry("2", "worker-2", LedgerPhase::Merged),
    ]);
    let long_ago = now() - Duration::days(20);
    let observed = Observations {
        prs: vec![
            PrObservation {
                task_id: Some("1".into()),
                url: Some("https://pr/1".into()),
                state: "MERGED".into(),
                merged_at: Some(long_ago),
                ..PrObservation::default()
            },
            PrObservation {
                task_id: Some("2".into()),
                url: Some("https://pr/2".into()),
                state: "MERGED".into(),
                merged_at: Some(long_ago),
                ..PrObservation::default()
            },
        ],
        ..Observations::default()
    };
    let events = transitions(&previous, &next, &observed, now());
    assert!(events.is_empty());
}

/// The case the suppression must not break: an operator escalates, and
/// minutes later merges the pull request by hand. That is news, and stays
/// news, whether the phase before it was `escalated` or `failed`.
#[test]
fn a_fresh_hand_merge_after_escalation_or_failure_still_notifies() {
    for phase in [LedgerPhase::Escalated, LedgerPhase::Failed] {
        let previous = ledger(vec![entry("1", "worker-1", phase)]);
        let next = ledger(vec![entry("1", "worker-1", LedgerPhase::Merged)]);
        let observed = merged_pr_observation("1", now() - Duration::minutes(2));
        let events = transitions(&previous, &next, &observed, now());
        assert_eq!(reasons(&events), ["merged"], "phase {phase:?}");
    }
}

/// No merge time at all — an old `gh`, or an observation stream this pass
/// never matched to a pull request — must not be read as staleness. A
/// real event is never dropped for want of a field to compare against.
#[test]
fn a_merge_with_no_reported_merge_time_still_notifies() {
    let previous = ledger(vec![entry("1", "worker-1", LedgerPhase::Escalated)]);
    let next = ledger(vec![entry("1", "worker-1", LedgerPhase::Merged)]);
    let events = transitions(&previous, &next, &Observations::default(), now());
    assert_eq!(reasons(&events), ["merged"]);
}
