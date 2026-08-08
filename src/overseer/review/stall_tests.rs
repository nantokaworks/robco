//! The stall finding's progress signal: a RUN worker can hold one ledger
//! phase — typically `claimed` — for hours by design while it works through a
//! subtree, so age-since-phase-entry alone cannot tell that apart from a
//! worker that stopped moving. A fresh commit on the entry's own branch can.

use super::tests::{ledger_with, live_entry, now, reasons_with_activity};
use crate::overseer::{config::OverseerConfig, ledger::LedgerPhase, monitor::BranchObservation};

/// The nex #528 shape: dispatched hours ago, still `claimed`, but the branch
/// keeps landing commits.
#[test]
fn a_run_worker_still_landing_commits_is_not_reported_as_stalled() {
    let config = OverseerConfig::default();
    let dispatched_at = now() - chrono::Duration::minutes(180);
    let ledger = ledger_with(vec![live_entry(
        "#528",
        LedgerPhase::Claimed,
        dispatched_at,
    )]);
    let branch_activity = vec![BranchObservation {
        task_id: "task-#528".into(),
        latest_commit_at: Some(now() - chrono::Duration::minutes(15)),
    }];

    let found = reasons_with_activity(&[], &ledger, &branch_activity, &config);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_worker_with_no_observed_commit_is_still_reported_as_stalled() {
    let config = OverseerConfig::default();
    let dispatched_at = now() - chrono::Duration::minutes(180);
    let ledger = ledger_with(vec![live_entry(
        "#528",
        LedgerPhase::Claimed,
        dispatched_at,
    )]);
    // The probe ran and found the branch, but nothing landed on it.
    let branch_activity = vec![BranchObservation {
        task_id: "task-#528".into(),
        latest_commit_at: None,
    }];

    let found = reasons_with_activity(&[], &ledger, &branch_activity, &config);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("stalled: #528 has been claimed for 180m"));
}

/// A commit that predates the claim is prior work the branch already carried,
/// not progress made under it — it must not reset the clock.
#[test]
fn a_commit_older_than_dispatch_does_not_reset_the_stall_clock() {
    let config = OverseerConfig::default();
    let dispatched_at = now() - chrono::Duration::minutes(180);
    let ledger = ledger_with(vec![live_entry(
        "#528",
        LedgerPhase::Claimed,
        dispatched_at,
    )]);
    let branch_activity = vec![BranchObservation {
        task_id: "task-#528".into(),
        latest_commit_at: Some(dispatched_at - chrono::Duration::minutes(30)),
    }];

    let found = reasons_with_activity(&[], &ledger, &branch_activity, &config);
    assert_eq!(found.len(), 1, "{found:?}");
}
