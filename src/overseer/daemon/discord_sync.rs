//! The daemon pass's Discord plumbing: keeping the bot's lifecycle in step
//! with the reloaded config, and draining the ledger requests the bot queued.
//! Split out of `daemon.rs`, which keeps the pass ordering itself.

use crate::{
    Result,
    config::Config,
    overseer::{
        discord::{self, BotGuard, ledger_requests::LedgerRequest},
        ledger::Ledger,
        logging, session,
    },
};
use std::sync::mpsc::{Receiver, Sender};

pub(super) fn sync_discord(
    guard: &mut Option<BotGuard>,
    logged_error: &mut Option<String>,
    config: &Config,
    ledger_requests: &Sender<LedgerRequest>,
) {
    let discord_config = &config.overseer.discord;
    if !discord_config.enabled {
        *guard = None;
        *logged_error = None;
    } else if let Some(guard) = guard {
        guard.update_config(discord_config.clone());
        *logged_error = None;
    } else {
        let env_file = session::env::env_file_path(config);
        match discord::start(
            discord_config.clone(),
            env_file.as_deref(),
            ledger_requests.clone(),
        ) {
            Ok(started) => {
                *guard = Some(started);
                *logged_error = None;
            }
            // The start attempt repeats every pass so a config edit is picked
            // up, but the same failure repeated is pure noise (dropr:390 saw
            // thousands of identical lines) — log only when the reason
            // changes.
            Err(error) => {
                if logged_error.as_ref() != Some(&error) {
                    eprintln!("overseer: Discord bot disabled: {error}");
                    *logged_error = Some(error);
                }
            }
        }
    }
}

/// A `!run <task>` request pulled off the queue. `apply_ledger_requests`
/// cannot launch it itself — that needs `Config`, `now`, and the
/// post-reconcile ledger, none of which it has — so it hands these back for
/// the caller to feed to `dispatch::run_named`.
///
/// Also the shape `RuntimeRequest::RunTask` (a named launch queued from
/// outside the daemon process, e.g. the TUI — dropr:470) converts into via
/// [`Self::from_runtime_request`], so the daemon's tick loop feeds both
/// transports through the one loop.
pub(super) struct PendingRun {
    pub(super) task: String,
    /// Discord's own user id, when this run came from Discord. `None` for a
    /// transport with no such identity (currently the TUI).
    pub(super) user_id: Option<String>,
    /// Attributed in the refusal decision entry's `source` field — `"discord"`
    /// or the `RuntimeRequest::RunTask::source` a non-Discord caller queued.
    pub(super) source: String,
}

impl PendingRun {
    pub(super) fn from_runtime_request(run: crate::overseer::runtime_request::PendingRun) -> Self {
        Self {
            task: run.task,
            user_id: None,
            source: run.source,
        }
    }
}

pub(super) fn apply_ledger_requests(
    ledger: &mut Ledger,
    requests: &Receiver<LedgerRequest>,
) -> Result<Vec<PendingRun>> {
    let mut pending_runs = Vec::new();
    while let Ok(request) = requests.try_recv() {
        if let LedgerRequest::Run { task, user_id } = request {
            pending_runs.push(PendingRun {
                task,
                user_id: Some(user_id),
                source: "discord".into(),
            });
            continue;
        }
        let (task, user_id) = request.attribution();
        let task = task.to_string();
        let user_id = user_id.to_string();
        if let Err(error) = discord::ledger_requests::apply(ledger, request) {
            let mut entry = logging::DecisionEntry::new(
                logging::DecisionKind::Hold,
                format!("Discord ledger request refused: {error}"),
            );
            entry.task = Some(task);
            entry.user_id = Some(user_id);
            entry.source = Some("discord".into());
            logging::append(&entry)?;
        }
    }
    Ok(pending_runs)
}
