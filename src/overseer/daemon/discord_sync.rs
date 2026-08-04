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

pub(super) fn apply_ledger_requests(
    ledger: &mut Ledger,
    requests: &Receiver<LedgerRequest>,
) -> Result<()> {
    while let Ok(request) = requests.try_recv() {
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
    Ok(())
}
