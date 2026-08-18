mod check_rollup;
mod discord_events;
mod discord_sync;
mod external_prs;
mod merge;
mod merge_allow;
mod merge_apply;
mod merge_concurrency;
mod merge_decision;
mod merge_delivery;
pub(crate) mod merge_dependency;
mod merge_escalation;
mod merge_evaluate;
mod merge_gate;
mod merge_hold;
mod merge_hold_recheck;
pub(crate) mod merge_pass_telemetry;
pub(crate) mod merge_queue;
mod merge_recovery;
mod merge_release_check;
mod merge_repo_pass;
mod merge_settle;
pub(crate) mod merge_state;
mod observations;
mod pr_facts;
mod protection;
pub(crate) mod pull_request;
mod repo_watch;
mod repo_watch_advisory;
mod repo_watch_dependabot;
mod repo_watch_task;
mod retention;
mod support;

pub(crate) use support::terminal;
use support::{account_failures, persist_drained_config};

use super::{
    dispatch,
    dispatch::dispatch_pass,
    exec::{PidGuard, append_jsonl, execute_actions},
    heartbeat, heartbeat_path,
    inbox::InboxReader,
    ledger::Ledger,
    logging,
    monitor::{ObservationError, ObservationSnapshot, reconcile},
    pidfile_path,
    review::ReviewPass,
    runtime_request, session, snapshots_path,
    triage::ExceptionQueue,
    wake::{self, Signals},
};
use crate::{Result, config::Config};
use chrono::Utc;
use std::{
    collections::{BTreeMap, BTreeSet},
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
    // sessions cannot authenticate fails triage and review silently, and the
    // operator's only signal used to be those passes quietly doing nothing.
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
    let mut review = ReviewPass::load()?;
    // Installed before the first pass so a wake or a signal delivered while a
    // pass is running is remembered rather than missed.
    let mut signals = Signals::install()?;
    // Repos whose unmaterialised-workspace skip was already recorded this
    // daemon run; deliberately in-memory so a restart logs each once more.
    let mut unmaterialised_logged = BTreeSet::new();
    // Agent id -> cleanup notes already logged this daemon run, so a stuck
    // worktree cleanup does not re-emit the same decision-log line every
    // pass. Deliberately in-memory, same rationale as `unmaterialised_logged`.
    let mut cleanup_notes_logged = BTreeMap::new();
    // Task id -> last logged per-candidate dispatch decision reason this
    // daemon run, so a candidate held on the same reason writes one line
    // instead of one per poll. Deliberately in-memory, same rationale as
    // `unmaterialised_logged`; `dispatch_pass` prunes a task id's entry once
    // it leaves the candidate list.
    let mut dispatch_hold_logged = BTreeMap::new();
    // Last logged global (not candidate-scoped) dispatch decision reason —
    // `dispatch_disabled`, `daily_limit`, `circuit_open`, or
    // `dropr_overlay_unavailable` — this daemon run. `None` once a pass gets
    // past every global gate.
    let mut dispatch_global_hold_logged = None;
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
        let pending_runs = discord_sync::apply_ledger_requests(&mut ledger, &ledger_request_rx)?;
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
        // Same best-effort tolerance as the other-PR discovery pass above: a
        // repo health watch failure must not interrupt dispatch or merging.
        if let Err(error) = repo_watch::watch_pass(&config, now) {
            logging::log_message(None, &format!("repo watch failed: {error}"))?;
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
        // Read-only, and deliberately placed before the acting passes: it
        // reviews the board the pass inherited rather than the one this pass is
        // in the middle of changing.
        review.tick(&config, &next, &observed, now)?;
        let pulled = execute_actions(
            &actions,
            config.overseer.release_pipeline_enabled,
            &mut cleanup_notes_logged,
        )?;
        // Snapshot before the pass mutates `next.entries` in place: neither
        // `merge_apply::merge_now` nor `merge_decision::concluded` (the two
        // ways this pass reaches `Merged`) runs through `monitor::reconcile`,
        // so this before/after diff is the only place that transition is
        // ever seen. See `merge_release_check::newly_merged`.
        let before_merge = next.entries.clone();
        merge::auto_merge_pass(&config, &mut next, &mut protections, &pulled)?;
        execute_actions(
            &merge_release_check::newly_merged(&before_merge, &next.entries),
            config.overseer.release_pipeline_enabled,
            &mut cleanup_notes_logged,
        )?;
        // After the merge pass, not before: the recheck budget it reads
        // (`merge_hold_recheck::due`) only reflects this tick's escalations
        // once that pass has run.
        merge_escalation::sweep_stuck(&mut next, now, config.overseer.max_merge_hold_rechecks)?;
        dispatch_pass(
            &mut config,
            &mut next,
            now,
            &mut unmaterialised_logged,
            &mut dispatch_hold_logged,
            &mut dispatch_global_hold_logged,
        )?;
        // Named dispatch, not a normal pass: one candidate at a time, against
        // the same ledger and gate `dispatch_pass` just used, so a slot it
        // already spent this tick is respected here too. See `!run`
        // (dropr:465) and `dispatch::run_named`'s own doc comment.
        for pending in pending_runs {
            let outcome = dispatch::run_named(
                &config,
                &mut next,
                &pending.task,
                now,
                &mut unmaterialised_logged,
                &mut dispatch_global_hold_logged,
            );
            // A dispatched run's own decision entry (`worker spawned:operator_run`,
            // from `dispatch::run_named` -> `spawn_candidate`) already carries the
            // full record; this is only for the refused/failed case, so the
            // requesting operator is attributed the same way a refused Skip or
            // Retry already is below.
            let refusal = match &outcome {
                Ok(dispatch::RunOutcome::Dispatched(_)) => None,
                Ok(dispatch::RunOutcome::Refused(reason)) => Some(reason.clone()),
                Err(error) => Some(error.to_string()),
            };
            if let Some(reason) = refusal {
                let mut entry = logging::DecisionEntry::new(
                    logging::DecisionKind::Hold,
                    format!("!run refused: {reason}"),
                );
                entry.task = Some(pending.task);
                entry.user_id = Some(pending.user_id);
                entry.source = Some("discord".into());
                logging::append(&entry)?;
            }
        }
        // Last, so every pass above reads the board it was given: retention only
        // decides how much of the settled past the *next* pass inherits, and a
        // drop must never take an entry out from under the reconcile, notify,
        // triage, merge, or dispatch pass that is still reading it.
        retention::prune_pass(
            &mut next,
            &observed.registered_agents,
            &observed,
            now,
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

#[cfg(test)]
#[path = "daemon_tests.rs"]
mod tests;
