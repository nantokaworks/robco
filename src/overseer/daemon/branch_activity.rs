//! The task branch's own local commit history, read straight off each live
//! (or recently settled) entry's worktree.
//!
//! Split out of `external_state.rs` on its own so the dropr-task and pull
//! request probes stay separately readable from this one.

use super::super::COMMAND_TIMEOUT;
use super::external_state::worth_probing;
use crate::overseer::{
    exec::run_timeout,
    ledger::Ledger,
    monitor::{BranchObservation, ObservationError, Observations},
};
use chrono::{DateTime, Utc};
use std::process::Command;

/// Reads the local commit timestamp on every `worth_probing` entry's own task
/// branch.
///
/// Every live entry is probed, not only `Escalated` ones: the board review's
/// stall finding (`review::findings::stalls`) reads this to tell a worker that
/// is still landing commits apart from one that stopped moving, and it cannot
/// do that from an escalated-only sample. `monitor::apply::apply_escalation_resolution`
/// reads the same observations for its own, narrower purpose — whether an
/// escalated entry's branch moved *after* it escalated.
///
/// `entry.repo` is the worker's own worktree, not a shared checkout, so a new
/// commit made from the same tmux session that pushed it is already on this
/// local branch — no `git fetch` is needed to see it.
pub(super) fn gather_branch_activity(
    ledger: &Ledger,
    observations: &mut Observations,
    now: DateTime<Utc>,
) {
    for entry in ledger
        .entries
        .iter()
        .filter(|entry| worth_probing(entry, now))
    {
        match latest_commit_at(&entry.repo, &entry.branch) {
            Ok(latest_commit_at) => observations.branches.push(BranchObservation {
                task_id: entry.task_id.clone(),
                latest_commit_at,
            }),
            Err(message) => observations
                .errors
                .push(ObservationError::new(message).about(&entry.task_id, &entry.repo)),
        }
    }
}

/// The task branch's own last commit time, or `None` when the branch no
/// longer exists locally — the worktree was already cleaned up, which is not
/// a probe failure, only a branch with nothing left to say.
fn latest_commit_at(
    repo: &str,
    branch: &str,
) -> std::result::Result<Option<DateTime<Utc>>, String> {
    let mut command = Command::new("git");
    command.args(["-C", repo, "log", "-1", "--format=%cI", branch]);
    let output = run_timeout(command, COMMAND_TIMEOUT)
        .map_err(|error| format!("git log probe skipped: {error}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(DateTime::parse_from_rfc3339(text.trim())
        .ok()
        .map(|at| at.with_timezone(&Utc)))
}
