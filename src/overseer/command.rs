use std::{
    collections::{BTreeMap, HashSet},
    fs,
    process::Command,
    thread,
    time::{Duration, SystemTime},
};

use super::{
    config::OverseerConfig,
    exec::{process_alive, run_timeout},
    heartbeat_path, is_overseer_child,
    judge::JudgmentQueue,
    ledger::Ledger,
    logging::{self, DecisionEntry, DecisionKind},
    pidfile_path,
    review::ReviewPass,
    runtime_request::{self, RuntimeRequest},
};
use crate::{
    Result,
    cli::{OverseerArgs, OverseerCommand},
    config::Config,
    registry::Registry,
};

mod escalation;
mod service;
mod settings;

pub(crate) use escalation::escalate_workers;
use service::install_service;
pub(crate) use service::write_service_plist;
pub(crate) use settings::set_runtime;
use settings::{autonomy_level, daily_limit, protection_mode, protection_warning, set};

pub(crate) use super::ledger::{ActiveWorkers, terminal};

pub fn run(args: OverseerArgs, config: &Config) -> Result<()> {
    match args.command {
        OverseerCommand::Run => unreachable!("async overseer run handled by main"),
        OverseerCommand::Status => status(config),
        OverseerCommand::Stop => stop(),
        OverseerCommand::Set(args) => set(config, args.setting, args.value.enabled()),
        OverseerCommand::DailyLimit(args) => daily_limit(args.value),
        OverseerCommand::Protection(args) => protection_mode(args.mode),
        OverseerCommand::Autonomy(args) => autonomy_level(args.level),
        OverseerCommand::Panic => panic_stop(),
        OverseerCommand::InstallService => install_service(),
    }
}

fn status(config: &Config) -> Result<()> {
    let ledger = Ledger::load()?;
    let judgments = JudgmentQueue::load()?;
    let pid = read_pid();
    let heartbeat_age = fs::metadata(heartbeat_path()?)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| SystemTime::now().duration_since(modified).ok());
    let healthy = daemon_healthy(config.overseer.poll_interval_secs);
    let active = ledger.active_workers();
    let mut phases = BTreeMap::new();
    for entry in &ledger.entries {
        *phases.entry(entry.phase.label()).or_insert(0usize) += 1;
    }
    println!(
        "daemon: {} pid={} heartbeat={}",
        if healthy { "healthy" } else { "down/stale" },
        pid.map_or_else(|| "-".into(), |pid| pid.to_string()),
        heartbeat_age.map_or_else(|| "missing".into(), |age| format!("{}s", age.as_secs()))
    );
    println!(
        "{}",
        toggle_line(
            &config.overseer,
            ledger.counters.consecutive_failures >= config.overseer.failure_circuit_threshold
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
        config.overseer.review_profile.as_deref().map_or_else(
            || "disabled".to_string(),
            |profile| format!(
                "every {}m via {profile}",
                config.overseer.review_interval_mins
            )
        )
    ))
}

fn daemon_healthy(poll_interval_secs: u64) -> bool {
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

fn stop() -> Result<()> {
    let Some(pid) = read_pid() else {
        println!("overseer is not running");
        return Ok(());
    };
    let mut command = Command::new("kill");
    command.args(["-TERM", &pid.to_string()]);
    let output = run_timeout(command, Duration::from_secs(2))?;
    if !output.status.success() {
        return Err(std::io::Error::other("failed to signal overseer daemon").into());
    }
    for _ in 0..20 {
        if !process_alive(pid) {
            println!("overseer stopped");
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    println!("SIGTERM sent; overseer is still shutting down");
    Ok(())
}

fn panic_stop() -> Result<()> {
    panic_stop_attributed("cli", None)?;
    println!("overseer panic stop complete");
    Ok(())
}

pub(crate) fn panic_stop_attributed(source: &str, user_id: Option<&str>) -> Result<()> {
    let mut config = Config::load()?;
    config.overseer.dispatch_enabled = false;
    config.save()?;
    let registry = Registry::load()?;
    let mut killed_ids = HashSet::new();
    for agent in registry
        .repos
        .iter()
        .flat_map(|repo| &repo.agents)
        .filter(|agent| is_overseer_child(agent.parent_agent_id.as_deref()))
    {
        let mut command = Command::new("tmux");
        command.args(["kill-session", "-t", &format!("={}", agent.tmux_session)]);
        if run_timeout(command, Duration::from_secs(5)).is_ok() {
            killed_ids.insert(agent.id.clone());
        }
    }
    runtime_request::enqueue(RuntimeRequest::PanicEscalate {
        source: source.into(),
        agent_ids: killed_ids.into_iter().collect(),
        at: chrono::Utc::now(),
    })?;
    let mut entry = DecisionEntry::new(
        DecisionKind::Escalate,
        "panic stop: dispatch disabled and workers terminated",
    );
    entry.source = Some(source.into());
    entry.user_id = user_id.map(str::to_owned);
    logging::append(&entry)?;
    Ok(())
}

fn read_pid() -> Option<u32> {
    fs::read_to_string(pidfile_path().ok()?)
        .ok()?
        .trim()
        .parse()
        .ok()
}
fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}
/// Render the toggle summary line of `robco overseer status`.
///
/// Every toggle reported here must be one the daemon actually honours; a switch
/// that is only displayed invites the reader to blame it for an outage it has no
/// part in.
fn toggle_line(config: &OverseerConfig, circuit_open: bool) -> String {
    format!(
        "dispatch: {}  auto-merge: {} (protection: {})  autonomy: {}  merge-recovery: {}  circuit: {}",
        on_off(config.dispatch_enabled),
        on_off(config.auto_merge),
        config.protection_mode.label(),
        config.autonomy_level.label(),
        merge_recovery_state(config),
        if circuit_open { "open" } else { "closed" }
    )
}

/// Merge recovery reads as one setting, so its cap travels with its switch: an
/// operator who sees only `on` cannot tell how many handbacks a stuck pull
/// request still has before it reaches them.
fn merge_recovery_state(config: &OverseerConfig) -> String {
    if config.merge_recovery_enabled {
        format!("on (max {})", config.max_merge_recoveries)
    } else {
        "off".into()
    }
}
pub(crate) fn load_active_workers() -> Result<ActiveWorkers> {
    let raw = fs::read_to_string(crate::overseer::ledger_path()?)?;
    let ledger: Ledger = serde_json::from_str(&raw)?;
    Ok(ledger.active_workers())
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod tests;
