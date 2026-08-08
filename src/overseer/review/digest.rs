//! The bounded picture of recent Overseer state the board review reads.
//!
//! A daemon that has been up for a week has a decision log of any size, and the
//! reviewer runs on a clock. So the digest is capped in every dimension —
//! number of decisions, number of ledger entries, and the length of each reason
//! string — before it reaches either the finding rules or an LLM prompt. The
//! caps are the reason a long-running daemon cannot grow this prompt without
//! limit.

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::{cmp::Reverse, collections::BTreeMap};

use crate::overseer::{
    config::OverseerConfig,
    ledger::{Ledger, terminal},
    logging::{DecisionEntry, DecisionKind},
    monitor::BranchObservation,
};

/// Most recent decisions carried into a review.
pub(super) const MAX_DECISIONS: usize = 200;
/// Most ledger entries described, oldest first.
const MAX_ENTRIES: usize = 50;
/// Longest reason text kept per decision.
const MAX_REASON_CHARS: usize = 160;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(super) struct Digest {
    pub decisions: Vec<Decision>,
    pub entries: Vec<Entry>,
    pub counters: Counters,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(super) struct Decision {
    pub at: DateTime<Utc>,
    pub kind: DecisionKind,
    pub task: Option<String>,
    pub source: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(super) struct Entry {
    pub task: String,
    pub repo: String,
    pub phase: &'static str,
    /// Minutes since the worker last showed progress: the latest local commit
    /// on its own task branch, or `dispatched_at` when no commit was observed
    /// (no branch activity probe, or none newer than dispatch). A RUN worker
    /// can hold one ledger phase — typically `claimed` — for hours by design
    /// while it works through a subtree, so age-since-phase-entry alone cannot
    /// tell that apart from a worker that stopped moving; a fresh commit can.
    pub age_mins: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(super) struct Counters {
    pub consecutive_failures: u32,
    pub failure_circuit_threshold: u32,
    pub dispatched_today: u32,
    pub dispatch_enabled: bool,
    pub active_workers: usize,
}

pub(super) fn build(
    decisions: &[DecisionEntry],
    ledger: &Ledger,
    branch_activity: &[BranchObservation],
    config: &OverseerConfig,
    now: DateTime<Utc>,
) -> Digest {
    // The reviewer writes escalations into the same log it reads. Feeding its own
    // output back would let one finding restated every pass become evidence of a
    // repeating problem — the review diagnosing itself.
    let considered = decisions
        .iter()
        .filter(|entry| entry.source.as_deref() != Some(super::SOURCE))
        .collect::<Vec<_>>();
    let start = considered.len().saturating_sub(MAX_DECISIONS);
    Digest {
        decisions: considered[start..]
            .iter()
            .map(|entry| Decision {
                at: entry.at,
                kind: entry.kind,
                task: entry.task.clone(),
                source: entry.source.clone(),
                reason: truncate(&entry.reason),
            })
            .collect(),
        entries: live_entries(ledger, branch_activity, now),
        counters: Counters {
            consecutive_failures: ledger.counters.consecutive_failures,
            failure_circuit_threshold: config.failure_circuit_threshold,
            dispatched_today: ledger.counters.dispatched_today,
            dispatch_enabled: config.dispatch_enabled,
            active_workers: ledger.active_workers().count,
        },
    }
}

fn live_entries(
    ledger: &Ledger,
    branch_activity: &[BranchObservation],
    now: DateTime<Utc>,
) -> Vec<Entry> {
    // Last observation wins on a duplicate task id, which never happens in
    // practice — one probe per live ledger entry per pass — but a map read is
    // no less correct than a list scan if it ever did.
    let latest_commit_at: BTreeMap<&str, DateTime<Utc>> = branch_activity
        .iter()
        .filter_map(|observation| {
            observation
                .latest_commit_at
                .map(|at| (observation.task_id.as_str(), at))
        })
        .collect();
    let mut entries = ledger
        .entries
        .iter()
        .filter(|entry| !terminal(entry.phase))
        .map(|entry| {
            // A commit older than dispatch is prior work the branch already
            // carried, not progress made under this claim — only a commit
            // strictly after `dispatched_at` counts.
            let progress_at = latest_commit_at
                .get(entry.task_id.as_str())
                .copied()
                .filter(|at| *at > entry.dispatched_at)
                .unwrap_or(entry.dispatched_at);
            Entry {
                task: if entry.display_id.is_empty() {
                    entry.task_id.clone()
                } else {
                    entry.display_id.clone()
                },
                repo: entry.repo.clone(),
                phase: entry.phase.label(),
                age_mins: (now - progress_at).num_minutes().max(0),
            }
        })
        .collect::<Vec<_>>();
    // Oldest first, so the cap drops the entries least likely to be stalled.
    entries.sort_by_key(|entry| (Reverse(entry.age_mins), entry.task.clone()));
    entries.truncate(MAX_ENTRIES);
    entries
}

fn truncate(reason: &str) -> String {
    let collapsed = reason.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= MAX_REASON_CHARS {
        return collapsed;
    }
    collapsed
        .chars()
        .take(MAX_REASON_CHARS)
        .chain("…".chars())
        .collect()
}
