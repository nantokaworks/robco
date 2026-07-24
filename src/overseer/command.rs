use std::{
    collections::{BTreeMap, HashSet},
    fs,
    process::Command,
    thread,
    time::{Duration, SystemTime},
};

use super::{
    exec::{process_alive, run_timeout},
    heartbeat_path, is_overseer_child,
    ledger::{Ledger, LedgerPhase},
    logging::{self, DecisionEntry, DecisionKind},
    pidfile_path,
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
use settings::{daily_limit, protection_mode, protection_warning, set};

pub(crate) use super::ledger::{ActiveWorkers, terminal};

pub fn run(args: OverseerArgs, config: &Config) -> Result<()> {
    match args.command {
        OverseerCommand::Run => unreachable!("async overseer run handled by main"),
        OverseerCommand::Status => status(config),
        OverseerCommand::Stop => stop(),
        OverseerCommand::Set(args) => set(config, args.setting, args.value.enabled()),
        OverseerCommand::DailyLimit(args) => daily_limit(args.value),
        OverseerCommand::Protection(args) => protection_mode(args.mode),
        OverseerCommand::Panic => panic_stop(),
        OverseerCommand::InstallService => install_service(),
    }
}

fn status(config: &Config) -> Result<()> {
    let ledger = Ledger::load()?;
    let pid = read_pid();
    let heartbeat_age = fs::metadata(heartbeat_path()?)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| SystemTime::now().duration_since(modified).ok());
    let healthy = daemon_healthy(config.overseer.poll_interval_secs);
    let active = ledger.active_workers();
    let mut phases = BTreeMap::new();
    for entry in &ledger.entries {
        *phases.entry(phase_name(entry.phase)).or_insert(0usize) += 1;
    }
    println!(
        "daemon: {} pid={} heartbeat={}",
        if healthy { "healthy" } else { "down/stale" },
        pid.map_or_else(|| "-".into(), |pid| pid.to_string()),
        heartbeat_age.map_or_else(|| "missing".into(), |age| format!("{}s", age.as_secs()))
    );
    println!(
        "overseer: {}  dispatch: {}  auto-merge: {} (protection: {})  circuit: {}",
        on_off(config.overseer.enabled),
        on_off(config.overseer.dispatch_enabled),
        on_off(config.overseer.auto_merge),
        config.overseer.protection_mode.label(),
        if ledger.counters.consecutive_failures >= config.overseer.failure_circuit_threshold {
            "open"
        } else {
            "closed"
        }
    );
    println!(
        "today: {}/{}  workers: {}/{}  per-repo cap: {}",
        ledger.counters.dispatched_today,
        crate::overseer::dispatch::format_dispatch_limit(config.overseer.daily_dispatch_limit),
        active.count,
        config.overseer.max_workers,
        config.overseer.per_repo_limit
    );
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
    Ok(())
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
pub(crate) fn load_active_workers() -> Result<ActiveWorkers> {
    let raw = fs::read_to_string(crate::overseer::ledger_path()?)?;
    let ledger: Ledger = serde_json::from_str(&raw)?;
    Ok(ledger.active_workers())
}
fn phase_name(phase: LedgerPhase) -> &'static str {
    match phase {
        LedgerPhase::Dispatched => "dispatched",
        LedgerPhase::Claimed => "claimed",
        LedgerPhase::Working => "working",
        LedgerPhase::PrOpened => "pr_opened",
        LedgerPhase::Merged => "merged",
        LedgerPhase::Failed => "failed",
        LedgerPhase::Escalated => "escalated",
    }
}
