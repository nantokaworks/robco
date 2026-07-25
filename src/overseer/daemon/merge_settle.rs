//! The per-repository merge barrier.
//!
//! A merge advances the base branch, so every other pull request in that
//! repository is now behind it and the primary worktree no longer holds what
//! `main` does. The next merge must wait for the post-merge
//! `git pull --ff-only` — which runs on a *later* pass, from the cleanup
//! sequence — before it may act on reads taken against the old base.
//!
//! This replaces the per-pass `merged_repos` set it grew out of, rather than
//! coexisting with it. That set was dropped when the pass returned, a poll
//! interval before the pull it was nominally waiting for even ran, so the hold
//! expired without ever depending on the thing it existed to depend on. Keeping
//! both would have put two names in `decisions.jsonl` for one condition,
//! distinguishable only by how long the daemon had been running.
//!
//! Repositories stay independent of one another: a merge in one never holds
//! another, because their base branches are unrelated.

use std::collections::{BTreeMap, HashSet};

use crate::overseer::ledger::MergeSettling;

/// Repositories currently waiting on their post-merge fast-forward, keyed by
/// repository path. Owned by the ledger, so the wait survives the pass.
pub(super) type Settling = BTreeMap<String, MergeSettling>;

/// Reason recorded for a pull request held because an earlier merge into the
/// same repository has not settled yet.
pub(super) const SETTLING: &str = "repo_merge_settling";

/// Reason recorded when the barrier is lifted without the pull ever landing.
/// Distinct from [`SETTLING`] on purpose: an operator reading the log has to be
/// able to tell a merge that waited its turn from one that went ahead onto a
/// base the daemon never confirmed.
pub(super) const SETTLE_CAP_REACHED: &str = "repo_merge_settle_cap_reached";

/// Reason recorded when the fast-forward landed and the barrier came down.
pub(super) const SETTLED: &str = "repo_merge_settled";

/// Whether a repository's barrier lets this pass merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Barrier {
    /// Nothing is settling: merge normally.
    Open,
    /// An earlier merge has not settled. Hold under [`SETTLING`].
    Held,
    /// The wait exceeded its bound and was abandoned. The merge proceeds, and
    /// the release is recorded under [`SETTLE_CAP_REACHED`].
    Lifted,
}

/// Raises the barrier for `repo`. Called on every merge the gate applies.
pub(super) fn begin(settling: &mut Settling, repo: &str) {
    settling.insert(repo.to_owned(), MergeSettling::default());
}

/// Lowers the barrier for every repository whose fast-forward just landed,
/// returning them so the caller can record it.
pub(super) fn settle(settling: &mut Settling, pulled: &HashSet<String>) -> Vec<String> {
    let landed = pulled
        .iter()
        .filter(|repo| settling.contains_key(*repo))
        .cloned()
        .collect::<Vec<_>>();
    for repo in &landed {
        settling.remove(repo);
    }
    landed
}

/// Ages every standing barrier by one pass.
///
/// Called once per auto-merge pass, before any of this pass's merges raise a
/// barrier of their own — a repository that entered settling this pass has
/// waited zero passes, not one.
pub(super) fn age(settling: &mut Settling) {
    for state in settling.values_mut() {
        state.passes_held = state.passes_held.saturating_add(1);
    }
}

/// What `repo`'s barrier says about merging into it now.
///
/// A [`Barrier::Lifted`] answer drops the state as it reports it: the bound has
/// been spent, and leaving it in place would re-report the release on every
/// later pass.
pub(super) fn barrier(settling: &mut Settling, repo: &str, max_passes: u32) -> Barrier {
    let Some(state) = settling.get(repo) else {
        return Barrier::Open;
    };
    if state.passes_held < max_passes {
        return Barrier::Held;
    }
    settling.remove(repo);
    Barrier::Lifted
}

#[cfg(test)]
#[path = "merge_settle_tests.rs"]
mod tests;
