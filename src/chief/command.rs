use std::{
    collections::BTreeMap,
    fs,
    process::Command,
    thread,
    time::{Duration, SystemTime},
};

use super::{
    CHIEF_AGENT_ID, chief_home,
    exec::{process_alive, run_timeout},
    heartbeat_path,
    ledger::{Ledger, LedgerPhase},
    logging::{self, DecisionEntry, DecisionKind},
    pidfile_path,
};
use crate::{
    Result,
    cli::{ChiefArgs, ChiefCommand, ChiefSetting},
    config::Config,
    registry::Registry,
};

pub fn run(args: ChiefArgs, config: &Config) -> Result<()> {
    match args.command {
        ChiefCommand::Run => unreachable!("async chief run handled by main"),
        ChiefCommand::Status => status(config),
        ChiefCommand::Stop => stop(),
        ChiefCommand::Set(args) => set(args.setting, args.value.enabled()),
        ChiefCommand::Panic => panic_stop(),
        ChiefCommand::InstallService => install_service().map(|_| ()),
    }
}

fn status(config: &Config) -> Result<()> {
    let ledger = Ledger::load()?;
    let pid = read_pid();
    let alive = pid.is_some_and(process_alive);
    let heartbeat_age = fs::metadata(heartbeat_path()?)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| SystemTime::now().duration_since(modified).ok());
    let fresh = heartbeat_age.is_some_and(|age| {
        age <= Duration::from_secs(config.chief.poll_interval_secs.saturating_mul(2).max(5))
    });
    let active: Vec<_> = ledger
        .entries
        .iter()
        .filter(|entry| !terminal(entry.phase))
        .collect();
    let mut repos = BTreeMap::new();
    for entry in &active {
        *repos.entry(entry.repo.as_str()).or_insert(0usize) += 1;
    }
    let mut phases = BTreeMap::new();
    for entry in &ledger.entries {
        *phases.entry(phase_name(entry.phase)).or_insert(0usize) += 1;
    }
    println!(
        "daemon: {} pid={} heartbeat={}",
        if alive && fresh {
            "healthy"
        } else {
            "down/stale"
        },
        pid.map_or_else(|| "-".into(), |pid| pid.to_string()),
        heartbeat_age.map_or_else(|| "missing".into(), |age| format!("{}s", age.as_secs()))
    );
    println!(
        "chief: {}  dispatch: {}  auto-merge: {}  circuit: {}",
        on_off(config.chief.enabled),
        on_off(config.chief.dispatch_enabled),
        on_off(config.chief.auto_merge),
        if ledger.counters.consecutive_failures >= config.chief.failure_circuit_threshold {
            "open"
        } else {
            "closed"
        }
    );
    println!(
        "today: {}/{}  workers: {}/{}  per-repo cap: {}",
        ledger.counters.dispatched_today,
        config.chief.daily_dispatch_limit,
        active.len(),
        config.chief.max_workers,
        config.chief.per_repo_limit
    );
    println!("workers by repo: {repos:?}");
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
    Ok(())
}

fn stop() -> Result<()> {
    let Some(pid) = read_pid() else {
        println!("chief is not running");
        return Ok(());
    };
    let mut command = Command::new("kill");
    command.args(["-TERM", &pid.to_string()]);
    let output = run_timeout(command, Duration::from_secs(2))?;
    if !output.status.success() {
        return Err(std::io::Error::other("failed to signal chief daemon").into());
    }
    for _ in 0..20 {
        if !process_alive(pid) {
            println!("chief stopped");
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    println!("SIGTERM sent; chief is still shutting down");
    Ok(())
}

fn set(setting: ChiefSetting, enabled: bool) -> Result<()> {
    set_runtime(setting, enabled)?;
    let label = match setting {
        ChiefSetting::Dispatch => "dispatch",
        ChiefSetting::AutoMerge => "auto-merge",
    };
    println!("{label}: {}", on_off(enabled));
    Ok(())
}

pub(crate) fn set_runtime(setting: ChiefSetting, enabled: bool) -> Result<()> {
    let mut config = Config::load()?;
    match setting {
        ChiefSetting::Dispatch => {
            config.chief.dispatch_enabled = enabled;
        }
        ChiefSetting::AutoMerge => {
            config.chief.auto_merge = enabled;
        }
    }
    config.save()?;
    if matches!(setting, ChiefSetting::Dispatch) && enabled {
        let mut ledger = Ledger::load()?;
        ledger.counters.consecutive_failures = 0;
        ledger.save()?;
    }
    Ok(())
}

fn panic_stop() -> Result<()> {
    panic_stop_attributed("cli", None)?;
    println!("chief panic stop complete");
    Ok(())
}

pub(crate) fn panic_stop_attributed(source: &str, user_id: Option<&str>) -> Result<()> {
    let mut config = Config::load()?;
    config.chief.dispatch_enabled = false;
    config.save()?;
    let registry = Registry::load()?;
    for agent in registry
        .repos
        .iter()
        .flat_map(|repo| &repo.agents)
        .filter(|agent| agent.parent_agent_id.as_deref() == Some(CHIEF_AGENT_ID))
    {
        let mut command = Command::new("tmux");
        command.args(["kill-session", "-t", &format!("={}", agent.tmux_session)]);
        let _ = run_timeout(command, Duration::from_secs(5));
    }
    let mut entry = DecisionEntry::new(
        DecisionKind::Escalate,
        "panic stop: dispatch disabled and workers terminated",
    );
    entry.source = Some(source.into());
    entry.user_id = user_id.map(str::to_owned);
    logging::append(&entry)?;
    Ok(())
}

pub(crate) fn install_service() -> Result<std::path::PathBuf> {
    let path = write_service_plist()?;
    println!("wrote {}", path.display());
    println!(
        "load with: launchctl bootstrap gui/$(id -u) {}",
        path.display()
    );
    Ok(path)
}

pub(crate) fn write_service_plist() -> Result<std::path::PathBuf> {
    let home = dirs::home_dir().ok_or(crate::Error::HomeDir)?;
    let dir = home.join("Library/LaunchAgents");
    fs::create_dir_all(&dir)?;
    let path = dir.join("com.robco.chief.plist");
    let executable = std::env::current_exe()?;
    let log = chief_home()?.join("chief.log");
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>com.robco.chief</string>
<key>ProgramArguments</key><array><string>{}</string><string>chief</string><string>run</string></array>
<key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
<key>StandardOutPath</key><string>{}</string><key>StandardErrorPath</key><string>{}</string>
</dict></plist>
"#,
        xml(&executable.to_string_lossy()),
        xml(&log.to_string_lossy()),
        xml(&log.to_string_lossy())
    );
    fs::write(&path, plist)?;
    Ok(path)
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
fn terminal(phase: LedgerPhase) -> bool {
    matches!(
        phase,
        LedgerPhase::Merged | LedgerPhase::Failed | LedgerPhase::Escalated
    )
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
fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
