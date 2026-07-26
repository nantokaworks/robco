//! What the surfaces read off the ledger.
//!
//! The ledger is a record of what Overseer did; these are the questions asked
//! of it — how much capacity is occupied, what the merge gate is declining, and
//! what it has given up on. Kept out of `ledger.rs` because a reader opening
//! that file is after the shape of the record and how it survives a restart,
//! and every new surface adds a question here rather than a field there.

use std::collections::BTreeMap;

use super::{Ledger, LedgerPhase, terminal};

/// Live workers counted globally and per repository.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct ActiveWorkers {
    pub count: usize,
    pub repos: BTreeMap<String, usize>,
}

/// A pull request the merge gate escalated and no longer acts on.
///
/// An escalation is the gate's last word on an entry: nothing merges it, and
/// nothing re-enters it until its pull request settles. Until this existed the
/// only trace was one line in `decisions.jsonl`, which is not a file an
/// operator reads by hand — so a green, mergeable pull request that Overseer
/// had given up on looked exactly like one it had never reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StuckMerge {
    pub display_id: String,
    pub pr_url: String,
    /// The gate reason the entry stopped on.
    pub reason: String,
}

impl StuckMerge {
    /// The line `robco overseer status` prints for it. The reason travels with
    /// the pull request, because "Overseer gave up on this" is only actionable
    /// together with what it gave up on.
    pub fn line(&self) -> String {
        format!(
            "stuck merge: {} {} ({})",
            self.display_id, self.pr_url, self.reason
        )
    }
}

impl Ledger {
    /// The workers occupying capacity right now. The dispatch gate and
    /// `robco overseer status` both read this one helper, so the count that
    /// enforces `max_workers` / `per_repo_limit` is the count the operator sees.
    ///
    /// Management mode is deliberately not a filter. Manual suppresses Overseer
    /// *intervention* — the worker belongs to a human, so it is never killed,
    /// restarted, or re-dispatched — but it still holds a worktree, a branch, a
    /// tmux session, and CPU in its repository. Exempting it from the caps would
    /// let a mode toggle free a slot the resources never released.
    pub fn active_workers(&self) -> ActiveWorkers {
        let mut repos: BTreeMap<String, usize> = BTreeMap::new();
        let mut count = 0;
        for entry in self.entries.iter().filter(|entry| !terminal(entry.phase)) {
            count += 1;
            *repos.entry(entry.repo.clone()).or_default() += 1;
        }
        ActiveWorkers { count, repos }
    }

    /// Live merge candidates the merge pass is declining because their worker is
    /// manual-managed.
    ///
    /// Read off the marker the merge pass itself writes rather than re-derived
    /// from the registry, so every surface reports the gate's own verdict instead
    /// of a second opinion that can disagree with it. Terminal entries are
    /// excluded: a pull request a human merged themselves is no longer something
    /// Overseer is holding back.
    pub fn manual_merge_skips(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.manual_merge_skip.is_some() && !terminal(entry.phase))
            .count()
    }

    /// Pull requests the merge gate escalated and stopped acting on.
    ///
    /// Read off the entry's own hold rather than re-derived, so the list names
    /// what the gate stopped on in the gate's own words. An escalation with no
    /// hold reason came from somewhere else — a worker that failed, a triage
    /// decision — and is not a merge the operator is being asked about; an entry
    /// with no pull request has nothing for them to look at either.
    pub fn stuck_merges(&self) -> Vec<StuckMerge> {
        self.entries
            .iter()
            .filter(|entry| entry.phase == LedgerPhase::Escalated)
            .filter_map(|entry| {
                Some(StuckMerge {
                    display_id: entry.display_id.clone(),
                    pr_url: entry.pr_url.clone()?,
                    reason: entry.merge_hold.reason.clone()?,
                })
            })
            .collect()
    }

    /// Merge failures a worker could have fixed that were left alone because
    /// merge recovery is switched off.
    ///
    /// Counted across every entry the ledger still holds, terminal ones included:
    /// an entry that escalated *because* nobody was handed its failure is the
    /// clearest evidence the setting costs something, and dropping it from the
    /// count would hide exactly the cases worth reading. The retention window is
    /// what bounds how far back this reaches.
    pub fn merge_recovery_drops(&self) -> u32 {
        self.entries.iter().fold(0, |total, entry| {
            total.saturating_add(entry.merge_recovery.dropped)
        })
    }
}
