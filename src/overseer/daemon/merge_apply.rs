//! The two steps that act on GitHub rather than reading it: updating a branch
//! that has fallen behind its base, and the merge itself.
//!
//! Kept beside the gate rather than in it because they are the only steps that
//! change state outside this process — everything before them is a read and a
//! decision, and a reader auditing what Overseer actually *did* to a repository
//! has one file to look at.

use std::process::Command;

use serde_json::Value;

use super::{
    COMMAND_TIMEOUT,
    merge_decision::{Halt, log_gated},
    merge_state,
    merge_state::{BehindPlan, MergeState},
};
use crate::{
    Result,
    config::Config,
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
pub(super) fn merge_state_cleared(
    entry: &mut LedgerEntry,
    url: &str,
    value: &Value,
    config: &Config,
) -> Option<Halt> {
    match merge_state::merge_state(value) {
        MergeState::Ready => None,
        MergeState::Held(raw) => Some(Halt::hold(merge_state::hold_reason(raw))),
        MergeState::Behind => match merge_state::plan_update(entry, &config.overseer) {
            BehindPlan::Update(flag) => {
                Some(match merge_state::run_update(&entry.repo, url, flag) {
                    Ok(()) => Halt::hold(merge_state::BRANCH_UPDATED),
                    Err(reason) => Halt::hold(reason),
                })
            }
            BehindPlan::Escalate => {
                entry.phase = LedgerPhase::Escalated;
                Some(Halt::escalate(merge_state::UPDATE_CAP_REACHED))
            }
        },
    }
}

/// Merges the pull request, recording the merge itself once GitHub accepted it.
///
/// The merge is the one decision recorded here rather than by the caller: it is the
/// only outcome that is not a failure, so it never reaches the recovery step.
pub(super) fn merge_now(
    entry: &mut LedgerEntry,
    url: &str,
    merge_strategy: &str,
    mode: ProtectionMode,
) -> Result<std::result::Result<(), Halt>> {
    let strategy = match merge_strategy {
        "merge" => "--merge",
        "rebase" => "--rebase",
        _ => "--squash",
    };
    let mut merge = Command::new("gh");
    merge
        .current_dir(&entry.repo)
        .args(["pr", "merge", url, strategy]);
    Ok(match run_timeout(merge, COMMAND_TIMEOUT) {
        Ok(output) if output.status.success() => {
            entry.phase = LedgerPhase::Merged;
            log_gated(
                entry,
                DecisionKind::Merge,
                strategy.trim_start_matches("--"),
                mode,
            )?;
            Ok(())
        }
        Ok(output) => Err(Halt::hold(format!("merge_exit:{}", output.status))),
        Err(error) => Err(Halt::hold(format!("merge_error:{error}"))),
    })
}
