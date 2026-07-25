use super::*;
use crate::overseer::{config::OverseerConfig, ledger::LedgerEntry};

fn failures(origin: FailureOrigin, count: u32) -> Vec<Action> {
    (0..count)
        .map(|index| Action::MarkFailed {
            task_id: format!("task-{index}"),
            reason: "failed".into(),
            origin,
        })
        .collect()
}

fn threshold() -> u32 {
    OverseerConfig::default().failure_circuit_threshold
}

#[test]
fn infra_failures_do_not_count_toward_circuit() {
    let previous = Ledger::default();
    let mut next = previous.clone();
    account_failures(
        &previous,
        &mut next,
        &failures(FailureOrigin::Infra, threshold()),
    );
    assert_eq!(next.counters.consecutive_failures, 0);
}

#[test]
fn worker_failures_reach_circuit_threshold() {
    let previous = Ledger::default();
    let mut next = previous.clone();
    account_failures(
        &previous,
        &mut next,
        &failures(FailureOrigin::Worker, threshold()),
    );
    assert_eq!(next.counters.consecutive_failures, threshold());
}

#[test]
fn mixed_failures_count_only_worker_origin() {
    let previous = Ledger::default();
    let mut next = previous.clone();
    let mut actions = failures(FailureOrigin::Infra, threshold());
    actions.extend(failures(FailureOrigin::Worker, 2));
    account_failures(&previous, &mut next, &actions);
    assert_eq!(next.counters.consecutive_failures, 2);
}

#[test]
fn newly_merged_task_resets_failure_counter() {
    let entry = |phase| LedgerEntry {
        task_id: "task-1".into(),
        display_id: "#1".into(),
        repo: "/repo".into(),
        agent_id: "worker-1".into(),
        branch: "task-1".into(),
        phase,
        dispatched_at: Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: None,
        branch_updates: 0,
        merge_recovery: Default::default(),
        manual_merge_skip: None,
    };
    let mut previous = Ledger {
        entries: vec![entry(LedgerPhase::Working)],
        ..Ledger::default()
    };
    previous.counters.consecutive_failures = threshold();
    let mut next = previous.clone();
    next.entries[0].phase = LedgerPhase::Merged;
    account_failures(&previous, &mut next, &failures(FailureOrigin::Worker, 1));
    assert_eq!(next.counters.consecutive_failures, 0);
}
