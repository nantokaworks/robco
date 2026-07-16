mod discord_events;
mod merge;
mod observations;

use super::{
    dispatch::dispatch_pass,
    exec::{PidGuard, append_jsonl, execute_actions},
    heartbeat_path,
    inbox::InboxReader,
    ledger::{Ledger, LedgerPhase},
    logging,
    monitor::{Action, ObservationSnapshot, reconcile},
    pidfile_path, snapshots_path,
    triage::ExceptionQueue,
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
    let (ledger_request_tx, ledger_request_rx) = mpsc::channel();
    let mut discord = None;
    sync_discord(&mut discord, &config.chief.discord, &ledger_request_tx);
    let mut ledger = Ledger::load()?;
    observations::adopt_registry_children(&mut ledger)?;
    ledger.save()?;
    let mut inbox = InboxReader::new()?;
    let mut protections = merge::ProtectionCache::default();
    let mut exceptions = ExceptionQueue::load()?;
    loop {
        let started = Instant::now();
        if let Ok(reloaded) = Config::load() {
            config = reloaded;
        } else {
            logging::log_message(None, "config reload failed; retaining previous config")?;
        }
        sync_discord(&mut discord, &config.chief.discord, &ledger_request_tx);
        apply_ledger_requests(&mut ledger, &ledger_request_rx)?;
        let now = Utc::now();
        let mut observed = observations::gather(&ledger, &mut inbox);
        if let Err(error) = append_jsonl(
            &snapshots_path()?,
            &ObservationSnapshot {
                at: now,
                observations: observed.clone(),
            },
        ) {
            observed
                .errors
                .push(format!("snapshot write failed: {error}"));
        }
        let (mut next, actions) = reconcile(&ledger, &observed, now, config.chief.stuck_after_mins);
        if config.chief.discord.enabled {
            discord_events::record(&ledger, &next, &observed)?;
        }
        account_failures(&ledger, &mut next, &actions);
        if config.chief.triage_enabled {
            exceptions.enqueue(&actions, &next, &observed)?;
            exceptions.tick(&config, &mut next)?;
        }
        execute_actions(&actions)?;
        merge::auto_merge_pass(&config, &mut next, &mut protections)?;
        dispatch_pass(&mut config, &mut next, now)?;
        // Persist decisions before removing their queue item. A crash before this
        // point replays the marker without repeating actions; after it, replay is
        // an idempotent ledger update until the queue acknowledgement is saved.
        next.save()?;
        exceptions.acknowledge_completion()?;
        inbox.commit()?;
        ledger = next;
        fs::write(heartbeat_path()?, now.to_rfc3339())?;
        let remaining =
            Duration::from_secs(config.chief.poll_interval_secs).saturating_sub(started.elapsed());
        if wait_for_shutdown(remaining).await? {
            return Ok(());
        }
    }
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
            Err(error) => eprintln!("chief: Discord bot disabled: {error}"),
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
    let failures = actions
        .iter()
        .filter(|action| matches!(action, Action::MarkFailed { .. }))
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

async fn wait_for_shutdown(duration: Duration) -> Result<bool> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            _ = tokio::time::sleep(duration) => Ok(false),
            result = tokio::signal::ctrl_c() => { result?; Ok(true) },
            _ = terminate.recv() => Ok(true),
        }
    }
    #[cfg(not(unix))]
    tokio::select! {
        _ = tokio::time::sleep(duration) => Ok(false),
        result = tokio::signal::ctrl_c() => { result?; Ok(true) },
    }
}
