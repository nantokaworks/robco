use std::{collections::HashSet, fs, process::Command, thread, time::Duration};

use super::{
    exec::{process_alive, run_timeout},
    is_overseer_child,
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
mod inbox;
mod service;
mod settings;
mod status;

pub(crate) use escalation::escalate_workers;
use inbox::clear_inbox;
use service::install_service;
#[cfg(target_os = "macos")]
pub(crate) use service::write_service_plist;
pub(crate) use settings::set_runtime;
use settings::{autonomy_level, daily_limit, protection_mode, set};
use status::status;

#[cfg(target_os = "macos")]
pub(crate) use super::ledger::ActiveWorkers;
#[cfg(target_os = "macos")]
use super::ledger::Ledger;
pub(crate) use super::ledger::terminal;

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
        OverseerCommand::ClearInbox => clear_inbox(),
        OverseerCommand::InstallService => install_service(),
    }
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
/// Read by the macOS service wizard only; the launchd bootstrap step is the
/// single caller.
#[cfg(target_os = "macos")]
pub(crate) fn load_active_workers() -> Result<ActiveWorkers> {
    let raw = fs::read_to_string(crate::overseer::ledger_path()?)?;
    let ledger: Ledger = serde_json::from_str(&raw)?;
    Ok(ledger.active_workers())
}
