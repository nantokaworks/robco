mod check_rollup;
mod discord_events;
mod merge;
mod merge_apply;
mod merge_decision;
mod merge_recovery;
mod merge_settle;
mod merge_state;
mod observations;
mod protection;
mod pull_request;
mod retention;

use super::{
    config_write,
    dispatch::dispatch_pass,
    exec::{PidGuard, append_jsonl, execute_actions},
    heartbeat_path,
    inbox::InboxReader,
    judge::JudgmentQueue,
    ledger::{Ledger, LedgerPhase},
    logging,
    monitor::{Action, FailureOrigin, ObservationError, ObservationSnapshot, reconcile},
    pidfile_path,
    review::ReviewPass,
    runtime_request, snapshots_path,
    triage::ExceptionQueue,
    wake::{self, Signals},
};
use crate::{Result, config::Config};
use chrono::Utc;
use std::{
    fs,
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);

pub async fn run_daemon() -> Result<()> {
    let _pid_guard = PidGuard::acquire(pidfile_path()?)?;
    let mut config = Config::load()?;
    // Logged once at startup rather than on every reload: the notice describes
    // the file, which does not change between passes, and the daemon merges
    // unattended — the log is where an operator finds out its strategy moved.
    if let Some(notice) = &config.merge_strategy_notice {
        logging::log_message(None, notice)?;
    }
    let (ledger_request_tx, ledger_request_rx) = mpsc::channel();
    let mut discord = None;
    sync_discord(&mut discord, &config.overseer.discord, &ledger_request_tx);
    let mut ledger = Ledger::load()?;
    observations::adopt_registry_children(&mut ledger)?;
    ledger.save()?;
    let mut inbox = InboxReader::new()?;
    let mut protections = protection::ProtectionCache::default();
    let mut exceptions = ExceptionQueue::load()?;
    let mut judgments = JudgmentQueue::load()?;
    let mut review = ReviewPass::load()?;
    // Installed before the first pass so a wake or a signal delivered while a
    // pass is running is remembered rather than missed.
    let mut signals = Signals::install()?;
    loop {
        let started = Instant::now();
        if let Ok(reloaded) = Config::load() {
            config = reloaded;
        } else {
            logging::log_message(None, "config reload failed; retaining previous config")?;
        }
        sync_discord(&mut discord, &config.overseer.discord, &ledger_request_tx);
        apply_ledger_requests(&mut ledger, &ledger_request_rx)?;
        match runtime_request::drain(&mut ledger, &mut config) {
            Ok(config_changed) => {
                if config_changed {
                    persist_drained_config(config.overseer.dispatch_enabled)?;
                }
            }
            Err(error) => logging::log_message(
                None,
                &format!("runtime request drain failed; retaining state: {error}"),
            )?,
        }
        let now = Utc::now();
        let mut observed = observations::gather(&ledger, &mut inbox);
        if let Err(error) = append_jsonl(
            &snapshots_path()?,
            &ObservationSnapshot {
                at: now,
                observations: observed.clone(),
            },
        ) {
            observed.errors.push(ObservationError::new(format!(
                "snapshot write failed: {error}"
            )));
        }
        let (mut next, actions) =
            reconcile(&ledger, &observed, now, config.overseer.stuck_after_mins);
        if config.overseer.discord.enabled {
            discord_events::record(&ledger, &next, &observed)?;
        }
        account_failures(&ledger, &mut next, &actions);
        if config.overseer.triage_enabled {
            exceptions.enqueue(&actions, &next, &observed)?;
            exceptions.tick(&config, &mut next)?;
        }
        judgments.tick(&config)?;
        // Read-only, and deliberately placed before the acting passes: it
        // reviews the board the pass inherited rather than the one this pass is
        // in the middle of changing.
        review.tick(&config, &next, now)?;
        let pulled = execute_actions(&actions)?;
        merge::auto_merge_pass(
            &config,
            &mut next,
            &mut protections,
            &mut judgments,
            &pulled,
        )?;
        dispatch_pass(&mut config, &mut next, now, &mut judgments)?;
        // Last, so every pass above reads the board it was given: retention only
        // decides how much of the settled past the *next* pass inherits, and a
        // drop must never take an entry out from under the reconcile, notify,
        // triage, merge, or dispatch pass that is still reading it.
        retention::prune_pass(
            &mut next,
            &observed.registered_agents,
            config.overseer.terminal_retention_per_repo,
        )?;
        // Persist decisions before removing their queue item. A crash before this
        // point replays the marker without repeating actions; after it, replay is
        // an idempotent ledger update until the queue acknowledgement is saved.
        next.save()?;
        exceptions.acknowledge_completion()?;
        inbox.commit()?;
        ledger = next;
        fs::write(heartbeat_path()?, now.to_rfc3339())?;
        if wake::wait_for_next_pass(
            &mut signals,
            started,
            Duration::from_secs(config.overseer.poll_interval_secs),
        )
        .await?
        {
            return Ok(());
        }
    }
}

/// Write back what the drained requests changed. `runtime_request::apply` only
/// ever flips `overseer.dispatch_enabled`, so the write narrows to that field
/// rather than serialising this pass's snapshot over an operator's edits.
fn persist_drained_config(dispatch_enabled: bool) -> Result<()> {
    if config_write::persist_dispatch_enabled(dispatch_enabled)? {
        logging::log_message(
            None,
            &format!("config rewritten: overseer.dispatch_enabled={dispatch_enabled}"),
        )?;
    }
    Ok(())
}

fn sync_discord(
    guard: &mut Option<super::discord::BotGuard>,
    config: &super::config::DiscordConfig,
    ledger_requests: &Sender<super::discord::ledger_requests::LedgerRequest>,
) {
    if !config.enabled {
        *guard = None;
    } else if let Some(guard) = guard {
        guard.update_config(config.clone());
    } else {
        match super::discord::start(config.clone(), ledger_requests.clone()) {
            Ok(started) => *guard = Some(started),
            Err(error) => eprintln!("overseer: Discord bot disabled: {error}"),
        }
    }
}

fn apply_ledger_requests(
    ledger: &mut Ledger,
    requests: &Receiver<super::discord::ledger_requests::LedgerRequest>,
) -> Result<()> {
    while let Ok(request) = requests.try_recv() {
        let (task, user_id) = request.attribution();
        let task = task.to_string();
        let user_id = user_id.to_string();
        if let Err(error) = super::discord::ledger_requests::apply(ledger, request) {
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

fn account_failures(previous: &Ledger, next: &mut Ledger, actions: &[Action]) {
    // Only worker-origin failures count; merges reset the streak, while re-arm
    // otherwise remains an operator action.
    let failures = actions
        .iter()
        .filter(|action| {
            matches!(
                action,
                Action::MarkFailed {
                    origin: FailureOrigin::Worker,
                    ..
                }
            )
        })
        .count() as u32;
    next.counters.consecutive_failures =
        next.counters.consecutive_failures.saturating_add(failures);
    let newly_merged = next.entries.iter().any(|entry| {
        entry.phase == LedgerPhase::Merged
            && previous
                .entries
                .iter()
                .find(|old| old.task_id == entry.task_id)
                .is_some_and(|old| old.phase != LedgerPhase::Merged)
    });
    if newly_merged {
        next.counters.consecutive_failures = 0;
    }
}

pub(crate) fn terminal(phase: LedgerPhase) -> bool {
    matches!(
        phase,
        LedgerPhase::Merged | LedgerPhase::Failed | LedgerPhase::Escalated
    )
}

#[cfg(test)]
#[path = "daemon_tests.rs"]
mod tests;
