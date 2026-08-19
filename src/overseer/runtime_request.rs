use std::{
    collections::HashSet,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use nanoid::nanoid;
use serde::{Deserialize, Serialize};

use super::{
    command::escalate_workers,
    daemon::pull_request,
    discord::ledger_requests::record_runtime_approval,
    ledger::{Ledger, OperatorOverride},
    runtime_requests_dir,
};
use crate::{Result, config::Config};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum RuntimeRequest {
    ResetCircuit {
        source: String,
        at: DateTime<Utc>,
    },
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
    /// A one-time operator decision to bypass the autonomy envelope's hard
    /// stop currently blocking one escalated pull request — granted by
    /// `mcp::tools::approve`'s fallback when the
    /// worker's own session that would otherwise receive the decision is no
    /// longer live to answer into. `target` is the ledger entry's `agent_id`
    /// or `display_id`, the same two keys `robco_approve` and the Inbox
    /// already key on. Applying it re-reads the pull request's current head
    /// rather than trusting one carried on the request, so a request that
    /// waited out a busy drain still names the revision the merge pass is
    /// about to see, not one taken at request time.
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

pub(crate) fn drain(ledger: &mut Ledger, config: &mut Config) -> Result<(bool, Vec<PendingRun>)> {
    drain_in(&runtime_requests_dir()?, ledger, config)
}

pub(crate) fn drain_in(
    dir: &Path,
    ledger: &mut Ledger,
    config: &mut Config,
) -> Result<(bool, Vec<PendingRun>)> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok((false, Vec::new())),
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

    let mut config_changed = false;
    let mut pending_runs = Vec::new();
    for path in paths {
        // A single unreadable/unremovable request must not abort the whole drain
        // (that would discard the accumulated config change and, worse, leave an
        // applied ResetCircuit file to replay every tick — silently re-zeroing the
        // failure streak and defeating the circuit). Handle each file resiliently.
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
            request => config_changed |= apply(ledger, config, request),
        }
        // Ack by removing the file. If removal fails, move it aside so an already
        // applied request cannot be replayed on the next tick.
        if let Err(error) = fs::remove_file(&path) {
            quarantine_applied(&path, &error);
        }
    }
    Ok((config_changed, pending_runs))
}

/// `overseer.dispatch_enabled` is the only config field a request may change:
/// the daemon's write-back reloads the file and copies just that field back, so
/// a variant mutating anything else would apply in memory and never persist.
/// Extend `config_write` alongside any variant that needs to.
pub(crate) fn apply(ledger: &mut Ledger, config: &mut Config, request: RuntimeRequest) -> bool {
    match request {
        RuntimeRequest::ResetCircuit { .. } => {
            ledger.counters.consecutive_failures = 0;
            if config.overseer.dispatch_enabled {
                false
            } else {
                config.overseer.dispatch_enabled = true;
                true
            }
        }
        RuntimeRequest::PanicEscalate { agent_ids, .. } => {
            escalate_workers(ledger, &agent_ids.into_iter().collect::<HashSet<_>>());
            false
        }
        // Nothing to apply: the pass that drains this request goes on to
        // observe the merge for itself, which is the whole point of waking it.
        RuntimeRequest::MergeCompleted { .. } => false,
        RuntimeRequest::OperatorMergeOverride { target, .. } => {
            grant_operator_override(ledger, &target);
            false
        }
        RuntimeRequest::MergeApproval {
            source,
            target,
            head,
            ..
        } => {
            record_runtime_approval(ledger, &target, &head, &source);
            false
        }
        // `drain_in` pulls this variant out before calling `apply` — see the
        // variant's own doc comment. Kept as an explicit no-op arm, not a
        // wildcard, so a future caller that bypasses `drain_in` fails safe
        // (nothing dispatches) instead of silently matching a wildcard.
        RuntimeRequest::RunTask { .. } => false,
    }
}

/// Finds the ledger entry `target` names (by `agent_id` or `display_id`) and
/// records a bypass scoped to its pull request's current head.
///
/// Silently a no-op when the entry is gone, carries no pull request yet, or
/// GitHub cannot be read right now — an operator override that missed its
/// moment is not a failure this drain should abort or retry over, the same
/// way [`apply`] already treats a `PanicEscalate` naming a since-vanished
/// agent id. The operator can simply approve again.
fn grant_operator_override(ledger: &mut Ledger, target: &str) {
    let Some(entry) = ledger
        .entries
        .iter_mut()
        .find(|entry| entry.agent_id == target || entry.display_id == target)
    else {
        return;
    };
    let Some(url) = entry.pr_url.clone() else {
        return;
    };
    let Ok(value) = pull_request::read(&entry.repo, &url) else {
        return;
    };
    entry.operator_override = Some(OperatorOverride {
        head: pull_request::head_sha(&value).to_owned(),
        granted_at: Utc::now(),
    });
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
