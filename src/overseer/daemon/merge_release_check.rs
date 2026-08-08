//! Finds the entries `merge::auto_merge_pass` just merged, so the daemon
//! loop can hand them to `overseer::release_pipeline`.
//!
//! `monitor::reconcile_entry` pushes an `Action::CheckReleasePipeline` when
//! it observes a pull request merged externally, but the auto-merge pass
//! reaches `Merged` two other ways — `merge_apply::merge_now`, and the
//! `PrConclusion::Merged` arm of `merge_decision::concluded` — and neither
//! runs through `reconcile`, so neither one is ever seen there. This module
//! is `daemon::run_daemon`'s own copy of that check for the auto-merge pass,
//! built the same way `discord_events::transitions` finds its own phase
//! changes: a before/after diff over ledger snapshots, since the pass
//! itself does not report which entries it moved.

use crate::overseer::{
    ledger::{LedgerEntry, LedgerPhase},
    monitor::Action,
};

/// One `Action::CheckReleasePipeline` per entry whose phase reached `Merged`
/// somewhere between `before` and `after` — matched by `task_id` and
/// `agent_id`, the same key `discord_events::transitions` matches on. An
/// entry already `Merged` in `before` is a pass re-running its cleanup, not
/// a new merge, so it is skipped the same way
/// `monitor::reconcile_entry`'s own check skips it.
pub(super) fn newly_merged(before: &[LedgerEntry], after: &[LedgerEntry]) -> Vec<Action> {
    after
        .iter()
        .filter(|entry| entry.phase == LedgerPhase::Merged)
        .filter(|entry| {
            let was_merged = before
                .iter()
                .find(|old| old.task_id == entry.task_id && old.agent_id == entry.agent_id)
                .is_some_and(|old| old.phase == LedgerPhase::Merged);
            !was_merged
        })
        .map(|entry| Action::CheckReleasePipeline {
            task_id: entry.task_id.clone(),
            repo: entry.repo.clone(),
            pr_url: entry.pr_url.clone(),
        })
        .collect()
}

#[cfg(test)]
#[path = "merge_release_check_tests.rs"]
mod tests;
