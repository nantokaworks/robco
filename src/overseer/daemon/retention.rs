//! What the ledger is allowed to remember about settled work.
//!
//! Nothing used to leave the ledger except a detached worker's entry, so every
//! entry that reached `merged`, `failed`, or `escalated` stayed in
//! `ledger.json` forever. The whole file is rewritten on every save and the
//! whole ledger is cloned on every reconcile pass, so that is slow-burn growth
//! charged to every pass — and once a history view reads settled entries, the
//! retention window silently becomes the history window.
//!
//! The window is a count per repository rather than an age, and it ranks on
//! `dispatched_at`. An entry now also records `settled_at`, but only for the
//! settlements observed since that field existed — every entry already terminal
//! when it arrived carries `None`, and ranking on a timestamp half the ledger
//! lacks would evict by upgrade date rather than by recency. `dispatched_at` is
//! the one instant every entry has. Repositories are counted separately so a
//! busy one cannot push another repository's history out of the ledger.
//!
//! Two entries are never dropped:
//!
//! * A non-terminal entry — it is live work, it holds a worktree, and this
//!   policy is about history.
//! * A terminal entry whose worker is still in the registry. `merged` cleanup is
//!   re-pushed on every pass for as long as the registry row survives, so
//!   dropping the entry first would leak the session and the worktree it was
//!   about to remove. It also keeps a `failed` or `escalated` entry — whose
//!   worktree is deliberately left standing for an operator — visible for as
//!   long as the thing it describes exists.
//!
//! Dropped history simply stops being remembered: once a task's entries have
//! all aged out, Overseer no longer remembers attempting it, and a later named
//! launch against the same task starts fresh rather than reading as a retry —
//! nothing gates on the recorded count any more (dropr:476). That is the
//! intended reading of a retention window — a task that has not been touched
//! in the last `terminal_retention_per_repo` settlements of its repository is
//! not being retried, it is being started again.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};

use super::observations::external_state::worth_probing;
use crate::Result;
use crate::overseer::{
    ledger::{Ledger, LedgerEntry, LedgerPhase, terminal},
    logging::{self, DecisionEntry, DecisionKind},
    monitor::{Observations, TASK_ABSENT},
};

/// Source recorded on a retention decision, so an operator can tell an entry the
/// window evicted from one a detach or a failure removed.
const SOURCE: &str = "retention";

/// Why one entry was dropped this pass — carried alongside it so
/// `prune_pass` can log the reason that actually applied instead of
/// re-deriving it from a ledger the entry has already left.
enum DropReason {
    /// Ranked outside its repository's `keep_per_repo` window — see
    /// `beyond_window`.
    Window,
    /// Confirmed gone — see `beyond_reach`.
    Reach,
}

/// Drop the settled entries that fall outside the retention window or that
/// nothing can act on any more, recording each drop in `decisions.jsonl`.
pub(super) fn prune_pass(
    ledger: &mut Ledger,
    registered: &[String],
    observations: &Observations,
    now: DateTime<Utc>,
    keep_per_repo: usize,
) -> Result<()> {
    for (entry, reason) in prune(ledger, registered, observations, now, keep_per_repo) {
        let message = match reason {
            DropReason::Window => format!(
                "{}: dropped {} ledger entry outside the {keep_per_repo}-entry retention window",
                entry.display_id,
                entry.phase.label()
            ),
            // The two facts `beyond_reach` accepts are spelled out here
            // rather than carried as a further reason variant: an operator
            // reading this line has no other place `daemon::retention`
            // explains what "gone" meant for this particular row.
            DropReason::Reach => format!(
                "{}: dropped escalated ledger entry — its dropr task or pull request no longer exists",
                entry.display_id
            ),
        };
        let mut decision = DecisionEntry::new(DecisionKind::Hold, message);
        decision.task = Some(entry.task_id);
        decision.repo = Some(entry.repo);
        decision.source = Some(SOURCE.into());
        decision.pr_url = entry.pr_url;
        logging::append(&decision)?;
    }
    Ok(())
}

/// Remove the droppable entries and return them, each with why it was
/// dropped, in ledger order. Survivors keep their relative order, so the
/// ledger stays the append-ordered log it was.
fn prune(
    ledger: &mut Ledger,
    registered: &[String],
    observations: &Observations,
    now: DateTime<Utc>,
    keep_per_repo: usize,
) -> Vec<(LedgerEntry, DropReason)> {
    let windowed = beyond_window(ledger, registered, keep_per_repo);
    let gone = beyond_reach(ledger, registered, observations, now);
    if windowed.is_empty() && gone.is_empty() {
        return Vec::new();
    }
    let mut dropped = Vec::with_capacity(windowed.len() + gone.len());
    let mut kept = Vec::with_capacity(ledger.entries.len());
    for (index, entry) in ledger.entries.drain(..).enumerate() {
        // `gone` takes precedence: an entry can rank inside the window and
        // still be confirmed gone, and "nothing left to act on" is the more
        // informative reason to report.
        if gone.contains(&index) {
            dropped.push((entry, DropReason::Reach));
        } else if windowed.contains(&index) {
            dropped.push((entry, DropReason::Window));
        } else {
            kept.push(entry);
        }
    }
    ledger.entries = kept;
    dropped
}

/// Indices of escalated entries confirmed to have nothing left for an
/// operator to act on: the dropr task that would have tracked them is gone,
/// or the pull request they once had is gone. Unlike [`beyond_window`] this
/// ignores `keep_per_repo` entirely — a confirmed-gone entry is not history
/// worth keeping regardless of how recently it settled.
///
/// Guarded the same way as `beyond_window`: an entry whose worker is still in
/// the registry is never dropped, so a worktree an operator leaves standing
/// is not pulled out from under a later cleanup pass.
fn beyond_reach(
    ledger: &Ledger,
    registered: &[String],
    observations: &Observations,
    now: DateTime<Utc>,
) -> HashSet<usize> {
    (0..ledger.entries.len())
        .filter(|&index| {
            let entry = &ledger.entries[index];
            entry.phase == LedgerPhase::Escalated
                && !registered.iter().any(|agent| agent == &entry.agent_id)
                && worth_probing(entry, now)
                && (task_confirmed_absent(entry, observations)
                    || pr_confirmed_gone(entry, observations))
        })
        .collect()
}

/// Whether this tick's dropr probe positively confirmed the entry's task no
/// longer exists — see `daemon::external_state::gather_repo_task_states` and
/// `monitor::TASK_ABSENT`.
fn task_confirmed_absent(entry: &LedgerEntry, observations: &Observations) -> bool {
    observations
        .tasks
        .iter()
        .any(|task| task.task_id == entry.task_id && task.state == TASK_ABSENT)
}

/// Whether this tick's `gh` probe positively confirmed the entry's branch
/// carries no pull request any more, for an entry that once had one.
///
/// An entry that never opened a pull request is excluded — that is a worker
/// killed mid-implementation, not a pull request that went away, and
/// `command::escalation`/`triage::completion` already leave it unreconsidered
/// for the same reason. `observations.errors` rules out the probe itself
/// having failed: `daemon::external_state::list_branch_prs` reports a
/// genuine absence as `Ok(None)` and a failed read as an error pushed there,
/// and only the former is evidence worth sweeping on.
fn pr_confirmed_gone(entry: &LedgerEntry, observations: &Observations) -> bool {
    entry.pr_url.is_some()
        && !observations
            .prs
            .iter()
            .any(|pr| pr.task_id.as_deref() == Some(entry.task_id.as_str()))
        && !observations
            .errors
            .iter()
            .any(|error| error.task_id.as_deref() == Some(entry.task_id.as_str()))
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
