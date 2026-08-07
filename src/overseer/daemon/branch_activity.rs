//! The task branch's own local commit history, read straight off an escalated
//! entry's worktree.
//!
//! Split out of `external_state.rs` on its own so the dropr-task and pull
//! request probes stay separately readable from this one.

use super::super::COMMAND_TIMEOUT;
use super::external_state::worth_probing;
use crate::overseer::{
    exec::run_timeout,
    ledger::{Ledger, LedgerPhase},
    monitor::{BranchObservation, ObservationError, Observations},
};
use chrono::{DateTime, Utc};
use std::process::Command;

/// Reads the local commit timestamp on an escalated entry's own task branch.
///
/// Scoped to `Escalated` rather than every `worth_probing` entry: a live
/// entry's branch changes on every commit a worker makes as a matter of
/// course, so reading it would only cost a `git log` nothing acts on. Only an
/// escalated entry needs to know whether the branch moved *after* it
/// escalated — see `monitor::apply::apply_escalation_resolution`.
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
        .filter(|entry| entry.phase == LedgerPhase::Escalated && worth_probing(entry, now))
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
