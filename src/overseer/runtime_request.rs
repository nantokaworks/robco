use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use nanoid::nanoid;
use serde::{Deserialize, Serialize};

mod apply_request;
use apply_request::apply;

use super::{ledger::Ledger, ledger_path, runtime_requests_dir};
use crate::{Result, registry::Registry};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum RuntimeRequest {
    PanicEscalate {
        source: String,
        agent_ids: Vec<String>,
        at: DateTime<Utc>,
    },
    /// A merge performed outside the daemon — currently the TUI. Applying it is
    /// deliberately a no-op: what the request buys is the wake, and the pass it
    /// wakes observes GitHub for itself rather than moving the ledger on the
    /// requester's word.
    MergeCompleted {
        source: String,
        repo: String,
        at: DateTime<Utc>,
    },
    /// A one-time operator merge request — granted by `mcp::tools::approve`'s
    /// fallback when the worker's own session that would otherwise receive
    /// the decision is no longer live to answer into. `target` is the ledger
    /// entry's `agent_id` or `display_id`, the same two keys `robco_approve`
    /// and the Inbox already key on. Applying it re-reads the pull request's
    /// current head rather than trusting one carried on the request, so a
    /// request that waited out a busy drain still names the revision the
    /// merge pass is about to see, not one taken at request time.
    OperatorMergeOverride {
        source: String,
        target: String,
        at: DateTime<Utc>,
    },
    /// An operator's one-time merge approval, queued by the TUI and scoped to
    /// the pull-request head or local branch tip the operator saw. Replaying
    /// the same serialized request records the same approval after a branch move.
    MergeApproval {
        source: String,
        target: String,
        head: String,
        at: DateTime<Utc>,
    },
    /// A named-task dispatch requested outside the daemon process — currently
    /// the TUI, launching a dropr task row from the repository INFO pane
    /// (dropr:470). The same shape as Discord's `!run <task>`, carried over
    /// this file queue instead of Discord's in-process channel because the
    /// TUI is a separate process from the daemon. `drain_in` pulls this
    /// variant out before it ever reaches [`apply`] — dispatching needs
    /// `Config`, `now`, and the post-reconcile `Ledger`, none of which `apply`
    /// has — and hands it back as a [`PendingRun`] for the caller to feed to
    /// `dispatch::run_named` alongside the pass's own polled dispatch, the
    /// same way `daemon::discord_sync::PendingRun` already does.
    RunTask {
        source: String,
        task: String,
        at: DateTime<Utc>,
    },
    /// An operator's own `u` action (TUI key or `robco_pr_update_branch` MCP
    /// tool) just brought a pull request's branch up to date with its base on
    /// GitHub's own side — see `crate::pr_update::update_behind`. `target` is
    /// the ledger entry's `agent_id` or `display_id`, the same two keys
    /// `robco_approve` and the Inbox already key on.
    ///
    /// Applying it resets the automated update budget
    /// (`LedgerEntry::branch_updates`) that `merge_state::plan_update` spends
    /// and, when the budget's own cap had escalated the entry, revives it so
    /// the next auto-merge pass looks at it again — the cap exists to stop an
    /// endless *automated* update loop, and an explicit operator action is
    /// not that loop (dropr:574).
    BranchUpdated {
        source: String,
        target: String,
        at: DateTime<Utc>,
    },
}

/// A [`RuntimeRequest::RunTask`] pulled off the queue, handed back to the
/// daemon tick loop to feed into `dispatch::run_named` — see the variant's
/// own doc comment for why `drain_in` cannot dispatch it directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingRun {
    pub(crate) task: String,
    pub(crate) source: String,
}

pub(crate) fn enqueue(request: RuntimeRequest) -> Result<()> {
    enqueue_in(&runtime_requests_dir()?, request)?;
    // The queue is drained at the top of every pass, so a request nobody
    // announces waits out the rest of the poll interval.
    super::wake::notify_daemon();
    Ok(())
}

pub(crate) fn enqueue_in(dir: &Path, request: RuntimeRequest) -> Result<()> {
    fs::create_dir_all(dir)?;
    let request_id = nanoid!();
    let path = dir.join(format!("{request_id}.json"));
    let temp_path = dir.join(format!("{request_id}.json.{}.tmp", nanoid!()));
    let raw = serde_json::to_string_pretty(&request)?;
    let written = fs::write(&temp_path, raw).and_then(|()| fs::rename(&temp_path, &path));
    if let Err(error) = written {
        let _ = fs::remove_file(temp_path);
        return Err(error.into());
    }
    Ok(())
}

pub(crate) fn drain(ledger: &mut Ledger, registry: Option<&Registry>) -> Result<Vec<PendingRun>> {
    drain_in(&runtime_requests_dir()?, &ledger_path()?, ledger, registry)
}

pub(crate) fn drain_in(
    dir: &Path,
    ledger_path: &Path,
    ledger: &mut Ledger,
    registry: Option<&Registry>,
) -> Result<Vec<PendingRun>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut paths = entries
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect::<Vec<_>>();
    // Requests apply in filename (nanoid) order, which is NOT chronological.
    // Variants must tolerate replay: most are commutative and idempotent, while
    // MergeApproval reaffirms the immutable head carried by that serialized
    // request instead of resolving a possibly newer branch tip. RunTask is
    // pulled out and dispatched once, the same as a `!run` request from Discord.
    // Preserve those replay guarantees when adding new variants.
    paths.sort();

    let mut pending_runs = Vec::new();
    for path in paths {
        // A single unreadable/unremovable request must not abort the whole drain
        // — that would leave an applied request file to replay every tick.
        // Handle each file resiliently.
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) => {
                eprintln!(
                    "warning: overseer runtime request {} unreadable, skipped this tick: {error}",
                    path.display()
                );
                continue;
            }
        };
        let request = match serde_json::from_str(&raw) {
            Ok(request) => request,
            Err(error) => {
                quarantine_corrupt(&path, &error);
                continue;
            }
        };
        match request {
            RuntimeRequest::RunTask { source, task, .. } => {
                pending_runs.push(PendingRun { task, source });
            }
            request => {
                apply(ledger, request, registry);
                // `apply` only changed `ledger` in memory, and this file is
                // about to be acked (deleted). Checkpoint the mutation now,
                // before the ack, so a daemon death before the pass's own
                // end-of-pass save cannot lose both the request's file and its
                // effect. On failure, leave the file in place: the mutation
                // already sitting in `ledger` is idempotent, so the next tick
                // simply reapplies and retries the save.
                if let Err(error) = ledger.save_to(ledger_path) {
                    eprintln!(
                        "warning: overseer runtime request {} applied but ledger checkpoint failed, left for retry: {error}",
                        path.display()
                    );
                    continue;
                }
            }
        }
        // Ack by removing the file. If removal fails, move it aside so an already
        // applied request cannot be replayed on the next tick.
        if let Err(error) = fs::remove_file(&path) {
            quarantine_applied(&path, &error);
        }
    }
    Ok(pending_runs)
}

fn quarantine_applied(path: &Path, error: &std::io::Error) {
    let aside = path.with_extension("json.applied");
    if fs::rename(path, &aside).is_err() {
        let _ = fs::remove_file(path);
    }
    eprintln!(
        "warning: overseer runtime request {} applied but could not be removed ({error}); moved aside to avoid replay",
        path.display()
    );
}

fn quarantine_corrupt(path: &Path, error: &serde_json::Error) {
    let corrupt_path = corrupt_path(path);
    if fs::rename(path, &corrupt_path).is_err() {
        let _ = fs::remove_file(path);
    }
    eprintln!(
        "warning: corrupt overseer runtime request {} skipped and moved aside: {error}",
        path.display()
    );
}

fn corrupt_path(path: &Path) -> PathBuf {
    path.with_extension("json.corrupt")
}

#[cfg(test)]
#[path = "runtime_request_tests.rs"]
mod tests;
