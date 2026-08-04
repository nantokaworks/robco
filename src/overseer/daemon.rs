mod check_rollup;
mod discord_events;
mod discord_sync;
mod external_prs;
mod merge;
mod merge_apply;
mod merge_decision;
mod merge_delivery;
pub(crate) mod merge_dependency;
mod merge_escalation;
mod merge_gate;
mod merge_hold;
mod merge_hold_recheck;
mod merge_judge_fail_safe;
mod merge_judge_gate;
pub(crate) mod merge_queue;
mod merge_recovery;
mod merge_settle;
pub(crate) mod merge_state;
mod observations;
mod protection;
mod pull_request;
mod retention;

use super::{
    config_write,
    dispatch::dispatch_pass,
    exec::{PidGuard, append_jsonl, execute_actions},
    heartbeat, heartbeat_path,
    inbox::InboxReader,
    judge::JudgmentQueue,
    ledger::{Ledger, LedgerPhase},
    logging,
    monitor::{Action, FailureOrigin, ObservationError, ObservationSnapshot, reconcile},
    pidfile_path,
    review::ReviewPass,
    runtime_request, session, snapshots_path,
    triage::ExceptionQueue,
    wake::{self, Signals},
};
use crate::{Result, config::Config};
use chrono::Utc;
use std::{
    collections::BTreeSet,
    sync::mpsc,
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
    // Same rationale as the merge-strategy notice above: the condition
    // describes the file, which does not change between passes.
    if let Some(notice) = &config.discord_legacy_notify_notice {
        logging::log_message(None, notice)?;
    }
    // Before the first pass, and never inside one: a daemon whose spawned
    // sessions cannot authenticate answers every judgment with a fail-safe, and
    // the operator's only signal used to be pull requests quietly piling up.
    if let Err(error) = session::preflight::run(&config) {
        logging::log_message(None, &format!("session preflight failed: {error}"))?;
    }
    let (ledger_request_tx, ledger_request_rx) = mpsc::channel();
    let mut discord = None;
    let mut discord_error = None;
    discord_sync::sync_discord(
        &mut discord,
        &mut discord_error,
        &config,
        &ledger_request_tx,
    );
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
    // Repos whose unmaterialised-workspace skip was already recorded this
    // daemon run; deliberately in-memory so a restart logs each once more.
    let mut unmaterialised_logged = BTreeSet::new();
    loop {
        let started = Instant::now();
        if let Ok(reloaded) = Config::load() {
            config = reloaded;
        } else {
            logging::log_message(None, "config reload failed; retaining previous config")?;
        }
        discord_sync::sync_discord(
            &mut discord,
            &mut discord_error,
            &config,
            &ledger_request_tx,
        );
        discord_sync::apply_ledger_requests(&mut ledger, &ledger_request_rx)?;
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
        let mut observed = observations::gather(&ledger, &mut inbox, now);
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
        // Best-effort and self-contained: a failure here must not interrupt
        // dispatch or merging, so it is logged rather than propagated.
        if let Err(error) = external_prs::refresh_pass(&ledger, now) {
            logging::log_message(None, &format!("other-PR discovery failed: {error}"))?;
        }
        let (mut next, actions) = reconcile(
            &ledger,
            &observed,
            now,
            config.overseer.stuck_after_mins,
            config.overseer.max_prerequisite_wait_hours,
        );
        if config.overseer.discord.enabled {
            discord_events::record(&ledger, &next, &observed, now)?;
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
        // After the merge pass, not before: the recheck budget it reads
        // (`merge_hold_recheck::due`) only reflects this tick's escalations
        // once that pass has run.
        merge_escalation::sweep_stuck(&mut next, now, config.overseer.max_merge_hold_rechecks)?;
        dispatch_pass(
            &mut config,
            &mut next,
            now,
            &mut judgments,
            &mut unmaterialised_logged,
        )?;
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
        heartbeat::write(&heartbeat_path()?, now)?;
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
