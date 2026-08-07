//! The two steps that act on GitHub rather than reading it: updating a branch
//! that has fallen behind its base, and the merge itself.
//!
//! Kept beside the gate rather than in it because they are the only steps that
//! change state outside this process — everything before them is a read and a
//! decision, and a reader auditing what Overseer actually *did* to a repository
//! has one file to look at.

use std::process::{Command, Output};

use serde_json::Value;

use super::{
    COMMAND_TIMEOUT,
    merge_decision::{Halt, log_gated},
    merge_queue::{self, Heads},
    merge_state,
    merge_state::{BehindPlan, MergeState},
};
use crate::{
    Result,
    config::{Config, MergeStrategy},
    git,
    overseer::{
        config::ProtectionMode,
        exec::run_timeout,
        ledger::{LedgerEntry, LedgerPhase},
        logging::DecisionKind,
    },
};

/// Acts on GitHub's own mergeability verdict. Returns `None` when the merge may proceed.
///
/// A branch that has merely fallen behind its base is updated and returned to the queue
/// so its required checks re-run against the new head; it is a recoverable state, so it
/// never marks the entry failed. Every other non-mergeable state is held under a reason
/// naming the state itself.
///
/// `heads` gates the update itself: only the first pull request of `entry.repo` to reach
/// this call in the current pass — its head of queue, see `merge_queue` — actually runs
/// `gh pr update-branch`. Every other pull request of the same repository that is also
/// `Behind` this pass is held under `merge_queue::WAITING_TURN` instead, because updating
/// it now would only be undone the moment the head merges.
pub(super) fn merge_state_cleared(
    entry: &mut LedgerEntry,
    url: &str,
    value: &Value,
    config: &Config,
    heads: &mut Heads,
) -> Option<Halt> {
    match merge_state::merge_state(value) {
        MergeState::Ready => {
            heads.claim(&entry.repo, &entry.agent_id);
            None
        }
        // Never claims the head slot: a pull request GitHub itself calls non-mergeable
        // for a reason other than falling behind is not making progress, so leaving the
        // slot open lets the next pull request in queue order claim it instead.
        MergeState::Held(raw) => Some(Halt::hold(merge_state::hold_reason(raw))),
        MergeState::Behind => {
            if !heads.claim(&entry.repo, &entry.agent_id) {
                return Some(Halt::hold(merge_queue::WAITING_TURN));
            }
            match merge_state::plan_update(entry, config) {
                BehindPlan::Update(flag) => {
                    Some(match merge_state::run_update(&entry.repo, url, flag) {
                        Ok(()) => Halt::hold(merge_state::BRANCH_UPDATED),
                        Err(reason) => Halt::hold(reason),
                    })
                }
                BehindPlan::Escalate => {
                    entry.phase = LedgerPhase::Escalated;
                    entry.worker_escalated = false;
                    Some(Halt::escalate(merge_state::UPDATE_CAP_REACHED))
                }
            }
        }
    }
}

/// Merges the pull request, recording the merge itself once GitHub accepted it.
///
/// The merge is the one decision recorded here rather than by the caller: it is the
/// only outcome that is not a failure, so it never reaches the recovery step.
pub(super) fn merge_now(
    entry: &mut LedgerEntry,
    url: &str,
    strategy: MergeStrategy,
    mode: ProtectionMode,
) -> Result<std::result::Result<(), Halt>> {
    let mut merge = Command::new("gh");
    merge
        .current_dir(&entry.repo)
        .args(["pr", "merge", url, strategy.gh_flag()]);
    Ok(match run_timeout(merge, COMMAND_TIMEOUT) {
        Ok(output) if output.status.success() => {
            entry.phase = LedgerPhase::Merged;
            log_gated(entry, DecisionKind::Merge, strategy.label(), mode)?;
            Ok(())
        }
        Ok(output) => Err(Halt::hold(hold_reason(strategy, &output))),
        Err(error) => Err(Halt::hold(format!("merge_error:{error}"))),
    })
}

/// Why the merge was held. A refusal robco can explain is recorded under its own
/// cause rather than the exit status, because the status says only that `gh`
/// failed — and this one is fixed by choosing another strategy, not by waiting.
fn hold_reason(strategy: MergeStrategy, output: &Output) -> String {
    match git::explain_merge_failure(strategy, &git::command_failure_text(output)) {
        Some(refusal) => format!("merge_refused:{}", refusal.reason),
        None => format!("merge_exit:{}", output.status),
    }
}

#[cfg(test)]
#[path = "merge_apply_tests.rs"]
mod tests;
