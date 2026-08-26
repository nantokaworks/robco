//! Deciding which dead agents the `CleanOnly` sequence may safely run
//! against without an operator's confirmation.
//!
//! An agent whose pull request merged outside robco (`gh pr merge`,
//! github.com) still leaves its tmux session to end on its own, the same as
//! any other finished worker — so `status::refresh_agent` reports it as the
//! ordinary `Status::Dead`, indistinguishable here from a session that
//! crashed. `crate::ui::tree::indicator` stops that from rendering as an
//! error once the ledger has observed the merge; this module is the other
//! half of dropr:563 — actually running the existing `CleanOnly` sequence
//! so the row leaves the tree the way an operator's own Land confirmation
//! already would, instead of sitting there dead forever.

use std::path::PathBuf;

use crate::{git, model::Status, overseer::ledger::Ledger, registry::Registry};

/// Agents whose pull request the ledger has observed merged — through
/// robco's own merge flow, or externally — while their session is
/// `Status::Dead`, and whose worktree carries no uncommitted or untracked
/// changes. [`crate::ui::App::apply_status`] runs the existing `CleanOnly`
/// sequence against each one.
///
/// A dirty worktree is deliberately excluded here rather than left for
/// `CleanOnly` itself to refuse: `git::post_merge::Cleanup::remove_worktree`
/// force-removes a worktree it cannot cleanly remove once the branch's own
/// content is already in the base — exactly this situation, since the pull
/// request is already merged — so running that unattended would risk
/// discarding uncommitted work the moment a merge was observed, instead of
/// only when an operator's own confirmed keypress (the `L` Land flow)
/// accepts that risk. A worktree that no longer exists has nothing left to
/// discard, so it counts as clean here.
pub(super) fn merged_cleanup_candidates(
    registry: &Registry,
    ledger: &Ledger,
) -> Vec<(PathBuf, String)> {
    let mut candidates = Vec::new();
    for repo in &registry.repos {
        for agent in &repo.agents {
            if agent.status != Status::Dead || !ledger.observed_merged(&agent.id) {
                continue;
            }
            let worktree_clean = agent.worktree_missing
                || git::worktree_is_clean(&agent.worktree_path).unwrap_or(false);
            if worktree_clean {
                candidates.push((repo.path.clone(), agent.id.clone()));
            }
        }
    }
    candidates
}

#[cfg(test)]
#[path = "auto_cleanup_tests.rs"]
mod tests;
