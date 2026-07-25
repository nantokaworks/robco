//! What the ledger is allowed to remember about settled work.
//!
//! Nothing used to leave the ledger except a detached worker's entry, so every
//! entry that reached `merged`, `failed`, or `escalated` stayed in
//! `ledger.json` forever. The whole file is rewritten on every save and the
//! whole ledger is cloned on every reconcile pass, so that is slow-burn growth
//! charged to every pass — and once a history view reads settled entries, the
//! retention window silently becomes the history window.
//!
//! The window is a count per repository rather than an age, because a settled
//! entry records no settling time: `dispatched_at` is the only timestamp an
//! entry carries, and dating retention off it would evict a long-running task
//! that merged this morning ahead of a short one that failed last week.
//! Repositories are counted separately so a busy one cannot push another
//! repository's history out of the ledger.
//!
//! Two entries are never dropped:
//!
//! * A non-terminal entry — it is live work, it holds a worktree and a dispatch
//!   slot, and this policy is about history.
//! * A terminal entry whose worker is still in the registry. `merged` cleanup is
//!   re-pushed on every pass for as long as the registry row survives, so
//!   dropping the entry first would leak the session and the worktree it was
//!   about to remove. It also keeps a `failed` or `escalated` entry — whose
//!   worktree is deliberately left standing for an operator — visible for as
//!   long as the thing it describes exists.
//!
//! The retry cap is the one thing that reads dropped history: `max_retries_per_task`
//! counts a task's recorded entries, so a task whose entries have all aged out
//! is a task Overseer no longer remembers attempting, and a still-ready one may
//! be dispatched again. That is the intended reading of a retention window — a
//! task that has not been touched in the last `terminal_retention_per_repo`
//! settlements of its repository is not being retried, it is being started
//! again — and `skip_list` remains the durable way to say never.

use std::collections::{HashMap, HashSet};

use crate::Result;
use crate::overseer::{
    ledger::{Ledger, LedgerEntry, terminal},
    logging::{self, DecisionEntry, DecisionKind},
};

/// Source recorded on a retention decision, so an operator can tell an entry the
/// window evicted from one a detach or a failure removed.
const SOURCE: &str = "retention";

/// Drop the settled entries that fall outside the retention window, recording
/// each drop in `decisions.jsonl`.
pub(super) fn prune_pass(
    ledger: &mut Ledger,
    registered: &[String],
    keep_per_repo: usize,
) -> Result<()> {
    for entry in prune(ledger, registered, keep_per_repo) {
        let mut decision = DecisionEntry::new(
            DecisionKind::Hold,
            format!(
                "{}: dropped {} ledger entry outside the {keep_per_repo}-entry retention window",
                entry.display_id,
                entry.phase.label()
            ),
        );
        decision.task = Some(entry.task_id);
        decision.repo = Some(entry.repo);
        decision.source = Some(SOURCE.into());
        decision.pr_url = entry.pr_url;
        logging::append(&decision)?;
    }
    Ok(())
}

/// Remove the droppable entries and return them in ledger order. Survivors keep
/// their relative order, so the ledger stays the append-ordered log it was.
fn prune(ledger: &mut Ledger, registered: &[String], keep_per_repo: usize) -> Vec<LedgerEntry> {
    let evicted = beyond_window(ledger, registered, keep_per_repo);
    if evicted.is_empty() {
        return Vec::new();
    }
    let mut dropped = Vec::with_capacity(evicted.len());
    let mut kept = Vec::with_capacity(ledger.entries.len() - evicted.len());
    for (index, entry) in ledger.entries.drain(..).enumerate() {
        if evicted.contains(&index) {
            dropped.push(entry);
        } else {
            kept.push(entry);
        }
    }
    ledger.entries = kept;
    dropped
}

/// Indices of the entries this pass may forget: terminal, unregistered, and
/// ranked outside their repository's window.
///
/// Every terminal entry is ranked, including the registered ones the pass must
/// keep, so a worktree an operator leaves standing does not widen the window for
/// the rest of its repository.
fn beyond_window(ledger: &Ledger, registered: &[String], keep_per_repo: usize) -> HashSet<usize> {
    // 0 = unlimited, matching how the dispatch limits read the same value.
    if keep_per_repo == 0 {
        return HashSet::new();
    }
    let mut ranked: Vec<usize> = (0..ledger.entries.len())
        .filter(|index| terminal(ledger.entries[*index].phase))
        .collect();
    // Newest first, so the window keeps the most recent settlements. Ties fall
    // back to ledger order, where a later entry is the later dispatch.
    ranked.sort_by(|left, right| {
        ledger.entries[*right]
            .dispatched_at
            .cmp(&ledger.entries[*left].dispatched_at)
            .then(right.cmp(left))
    });
    let mut counted: HashMap<&str, usize> = HashMap::new();
    let mut evicted = HashSet::new();
    for index in ranked {
        let entry = &ledger.entries[index];
        let seen = counted.entry(entry.repo.as_str()).or_default();
        *seen += 1;
        if *seen > keep_per_repo && !registered.iter().any(|agent| agent == &entry.agent_id) {
            evicted.insert(index);
        }
    }
    evicted
}

#[cfg(test)]
#[path = "retention_tests.rs"]
mod tests;
