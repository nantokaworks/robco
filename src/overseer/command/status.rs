//! `robco overseer status`: the one read an operator gets of what the daemon is
//! doing, and the line builders that read is assembled from.

use std::{
    collections::BTreeMap,
    fs,
    time::{Duration, SystemTime},
};

use super::{on_off, read_pid, settings::protection_warning};
use crate::{
    Result,
    config::Config,
    overseer::{
        config::OverseerConfig, exec::process_alive, heartbeat, heartbeat_path,
        judge::JudgmentQueue, ledger::Ledger, logging, review::ReviewPass,
    },
};

pub(super) fn status(config: &Config) -> Result<()> {
    let ledger = Ledger::load()?;
    let judgments = JudgmentQueue::load()?;
    let pid = read_pid();
    let heartbeat = heartbeat_path()?;
    let heartbeat_age = fs::metadata(&heartbeat)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| SystemTime::now().duration_since(modified).ok());
    let daemon_version = heartbeat::recorded_version(&heartbeat);
    let healthy = daemon_healthy(config.overseer.poll_interval_secs);
    let active = ledger.active_workers();
    let mut phases = BTreeMap::new();
    for entry in &ledger.entries {
        *phases.entry(entry.phase.label()).or_insert(0usize) += 1;
    }
    println!(
        "{}",
        daemon_line(healthy, pid, heartbeat_age, daemon_version.as_deref())
    );
    println!(
        "{}",
        toggle_line(
            &config.overseer,
            ledger.counters.consecutive_failures >= config.overseer.failure_circuit_threshold,
            ledger.merge_recovery_drops()
        )
    );
    println!(
        "today: {}/{}  workers: {}/{}  per-repo cap: {}",
        ledger.counters.dispatched_today,
        crate::overseer::dispatch::format_dispatch_limit(config.overseer.daily_dispatch_limit),
        active.count,
        config.overseer.max_workers,
        config.overseer.per_repo_limit
    );
    println!("{}", llm_line(config, &judgments)?);
    // Read straight after the LLM budget: a judge that spent nothing today is
    // either idle or queued behind one long judgment, and only this line says
    // which.
    println!("{}", judgments.snapshot().summary());
    println!("workers by repo: {:?}", active.repos);
    println!("phases: {phases:?}");
    // Only when there is something to report: the line names an exception state,
    // and a pull request Overseer is deliberately leaving to its owner is the
    // one thing the phase counts above cannot distinguish from a stalled merge.
    let manual_merges = ledger.manual_merge_skips();
    if manual_merges != 0 {
        println!("merge-eligible, manual: {manual_merges}");
    }
    println!("skip list: {:?}", ledger.skip_list);
    println!("recent decisions:");
    for entry in logging::tail(10)? {
        println!(
            "  {} {:?} task={} repo={} {}",
            entry.at.to_rfc3339(),
            entry.kind,
            entry.task.as_deref().unwrap_or("-"),
            entry.repo.as_deref().unwrap_or("-"),
            entry.reason
        );
    }
    // First of the warnings, and gated on the daemon being up: a stale build is
    // the one state in which every line above is accurate and still describes a
    // daemon that has none of what was merged since it started.
    if healthy && let Some(warning) = heartbeat::drift(daemon_version.as_deref()) {
        println!("warning: {warning}");
    }
    if config.overseer.dispatch_enabled && !healthy {
        println!("warning: {}", crate::overseer::DISPATCH_WITHOUT_DAEMON_HINT);
    }
    if config.overseer.auto_merge
        && let Some(warning) = protection_warning(config.overseer.protection_mode)
    {
        println!("warning: {warning}");
    }
    // Both warnings are gated on auto-merge: the envelope only decides anything
    // while the merge pass runs, so warning about it otherwise would name a gate
    // that is not currently in the path.
    if config.overseer.auto_merge
        && let Some(warning) = config.overseer.autonomy_level.envelope_warning()
    {
        println!("warning: {warning}");
    }
    Ok(())
}

/// Today's LLM spend, per surface.
///
/// The board reviewer runs on a clock and would consume most of a shared budget
/// on its own, so it carries its own. Reporting the two counts separately is
/// what lets an operator see which surface exhausted which budget instead of
/// inferring it from a single number.
fn llm_line(config: &Config, judgments: &JudgmentQueue) -> Result<String> {
    let judge = judgments.llm_calls_today();
    let review = ReviewPass::load()?.calls_today();
    Ok(format!(
        "llm today: judge {judge}/{}  review {review}/{} ({})",
        config.overseer.daily_llm_budget,
        config.overseer.daily_review_budget,
        review_state(&config.overseer)
    ))
}

/// How the board review is running.
///
/// The pass itself always runs on its interval, and its deterministic findings
/// with it; `review_profile` decides only whether a reviewer model reads the same
/// digest afterwards. The line used to print `disabled` for a missing profile,
/// which read as "the review is off" — and was, which is the bug this wording
/// outlived. The two states are now named separately, because an operator seeing a
/// quiet board needs to know whether nothing was found or nothing looked.
fn review_state(config: &OverseerConfig) -> String {
    let interval = config.review_interval_mins;
    config.review_profile.as_deref().map_or_else(
        || format!("findings every {interval}m, no reviewer model"),
        |profile| format!("every {interval}m via {profile}"),
    )
}

/// The daemon's identity line: whether it is up, which process it is, when it
/// last reported, and — the one thing a running daemon cannot otherwise be
/// asked — which build it started from. A daemon keeps that build until the
/// service restarts, so `healthy` alone never says whether a merged fix is live.
fn daemon_line(
    healthy: bool,
    pid: Option<u32>,
    heartbeat_age: Option<Duration>,
    daemon_version: Option<&str>,
) -> String {
    format!(
        "daemon: {} pid={} heartbeat={} version={}",
        if healthy { "healthy" } else { "down/stale" },
        pid.map_or_else(|| "-".into(), |pid| pid.to_string()),
        heartbeat_age.map_or_else(|| "missing".into(), |age| format!("{}s", age.as_secs())),
        daemon_version.unwrap_or("unknown")
    )
}

pub(super) fn daemon_healthy(poll_interval_secs: u64) -> bool {
    read_pid().is_some_and(process_alive)
        && heartbeat_path()
            .ok()
            .and_then(|path| fs::metadata(path).ok())
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| {
                age <= Duration::from_secs(poll_interval_secs.saturating_mul(2).max(5))
            })
}

/// Render the toggle summary line of `robco overseer status`.
///
/// Every toggle reported here must be one the daemon actually honours; a switch
/// that is only displayed invites the reader to blame it for an outage it has no
/// part in.
fn toggle_line(config: &OverseerConfig, circuit_open: bool, recovery_drops: u32) -> String {
    format!(
        "dispatch: {}  auto-merge: {} (protection: {})  autonomy: {}  merge-recovery: {}  circuit: {}",
        on_off(config.dispatch_enabled),
        on_off(config.auto_merge),
        config.protection_mode.label(),
        config.autonomy_level.label(),
        merge_recovery_state(config, recovery_drops),
        if circuit_open { "open" } else { "closed" }
    )
}

/// Merge recovery reads as one setting, so its cap travels with its switch: an
/// operator who sees only `on` cannot tell how many handbacks a stuck pull
/// request still has before it reaches them.
///
/// Switched off, the number that matters is the opposite one — how many failures
/// a worker could have fixed went to nobody. Without it `off` is a flag; with it
/// the flag has a consequence attached, which is what an operator needs to decide
/// whether to switch it on. It is omitted while nothing has been dropped, so the
/// line names an exception rather than a permanent zero.
fn merge_recovery_state(config: &OverseerConfig, drops: u32) -> String {
    if config.merge_recovery_enabled {
        return format!("on (max {})", config.max_merge_recoveries);
    }
    if drops == 0 {
        return "off".into();
    }
    format!("off ({drops} dropped)")
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
