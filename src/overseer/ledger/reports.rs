//! Read-only counts over the ledger — the ones `robco overseer status`, the
//! dispatch gate, and the merge gate read off it. Split out of `ledger.rs` to
//! keep that file under this project's source file size limit, the same
//! reason `budgets.rs` and `phase.rs` are split out.

use std::collections::BTreeMap;

use super::{Ledger, holds_capacity, terminal};

/// Live workers counted globally and per repository.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct ActiveWorkers {
    pub count: usize,
    pub repos: BTreeMap<String, usize>,
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
        for entry in self.entries.iter().filter(|entry| holds_capacity(entry)) {
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

    /// Merges Discord's `!merge` queued an approval for while they were still
    /// waiting on the deterministic gate, and have not yet drained.
    ///
    /// Read by `robco overseer status --debug`, the same way
    /// [`Self::manual_merge_skips`] is, so an operator can see how many
    /// pending merges already carry their own approval rather than a future
    /// escalation.
    pub fn queued_merge_approvals(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.merge_approval.is_some() && !terminal(entry.phase))
            .count()
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
