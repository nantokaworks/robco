use std::{
    collections::BTreeMap,
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
    overseer_home, pidfile_path,
};
use crate::{
    Result,
    cli::{OverseerArgs, OverseerCommand, OverseerSetting},
    config::Config,
    registry::Registry,
};

pub fn run(args: OverseerArgs, config: &Config) -> Result<()> {
    match args.command {
        OverseerCommand::Run => unreachable!("async overseer run handled by main"),
        OverseerCommand::Status => status(config),
        OverseerCommand::Stop => stop(),
        OverseerCommand::Set(args) => set(config, args.setting, args.value.enabled()),
        OverseerCommand::Panic => panic_stop(),
        OverseerCommand::InstallService => install_service().map(|_| ()),
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
        if healthy { "healthy" } else { "down/stale" },
        pid.map_or_else(|| "-".into(), |pid| pid.to_string()),
        heartbeat_age.map_or_else(|| "missing".into(), |age| format!("{}s", age.as_secs()))
    );
    println!(
        "overseer: {}  dispatch: {}  auto-merge: {}  circuit: {}",
        on_off(config.overseer.enabled),
        on_off(config.overseer.dispatch_enabled),
        on_off(config.overseer.auto_merge),
        if ledger.counters.consecutive_failures >= config.overseer.failure_circuit_threshold {
            "open"
        } else {
            "closed"
        }
    );
    println!(
        "today: {}/{}  workers: {}/{}  per-repo cap: {}",
        ledger.counters.dispatched_today,
        config.overseer.daily_dispatch_limit,
        active.len(),
        config.overseer.max_workers,
        config.overseer.per_repo_limit
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
    if config.overseer.dispatch_enabled && !healthy {
        println!("warning: {}", crate::overseer::DISPATCH_WITHOUT_DAEMON_HINT);
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

fn set(config: &Config, setting: OverseerSetting, enabled: bool) -> Result<()> {
    set_runtime(setting, enabled)?;
    let label = match setting {
        OverseerSetting::Dispatch => "dispatch",
        OverseerSetting::AutoMerge => "auto-merge",
    };
    println!("{label}: {}", on_off(enabled));
    if matches!(setting, OverseerSetting::Dispatch)
        && enabled
        && !daemon_healthy(config.overseer.poll_interval_secs)
    {
        println!("warning: {}", crate::overseer::DISPATCH_WITHOUT_DAEMON_HINT);
    }
    Ok(())
}

pub(crate) fn set_runtime(setting: OverseerSetting, enabled: bool) -> Result<()> {
    let mut config = Config::load()?;
    match setting {
        OverseerSetting::Dispatch => {
            config.overseer.dispatch_enabled = enabled;
        }
        OverseerSetting::AutoMerge => {
            config.overseer.auto_merge = enabled;
        }
    }
    config.save()?;
    if matches!(setting, OverseerSetting::Dispatch) && enabled {
        let mut ledger = Ledger::load()?;
        ledger.counters.consecutive_failures = 0;
        ledger.save()?;
    }
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
    for agent in registry
        .repos
        .iter()
        .flat_map(|repo| &repo.agents)
        .filter(|agent| is_overseer_child(agent.parent_agent_id.as_deref()))
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

fn remove_legacy_service() -> Result<()> {
    let home = dirs::home_dir().ok_or(crate::Error::HomeDir)?;
    let legacy_plist = home.join("Library/LaunchAgents/com.robco.chief.plist");
    let mut id = Command::new("id");
    id.arg("-u");
    let output = run_timeout(id, Duration::from_secs(2))?;
    if !output.status.success() {
        return Err(crate::Error::Command {
            context: "look up user id for legacy launchd cleanup",
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    let uid = String::from_utf8_lossy(&output.stdout);
    let uid = uid.trim().parse::<u32>().map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid user id returned by `id -u`: {error}"),
        )
    })?;
    let mut command = Command::new("launchctl");
    command.args(["bootout", &format!("gui/{uid}/com.robco.chief")]);
    let output = run_timeout(command, Duration::from_secs(5))?;
    if !output.status.success() && !legacy_service_is_absent(output.status.code(), &output.stderr) {
        return Err(crate::Error::Command {
            context: "boot out legacy launchd service",
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    if legacy_plist.try_exists()? {
        fs::remove_file(legacy_plist)?;
    }
    Ok(())
}

fn legacy_service_is_absent(status: Option<i32>, stderr: &[u8]) -> bool {
    status == Some(3) && String::from_utf8_lossy(stderr).contains("No such process")
}

pub(crate) fn write_service_plist() -> Result<std::path::PathBuf> {
    remove_legacy_service()?;
    let home = dirs::home_dir().ok_or(crate::Error::HomeDir)?;
    let dir = home.join("Library/LaunchAgents");
    fs::create_dir_all(&dir)?;
    let path = dir.join("com.robco.overseer.plist");
    let executable = std::env::current_exe()?;
    let log = overseer_home()?.join("overseer.log");
    let path_env = service_path_env(std::env::var("PATH").ok().as_deref(), &home);
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>com.robco.overseer</string>
<key>ProgramArguments</key><array><string>{}</string><string>overseer</string><string>run</string></array>
<key>EnvironmentVariables</key><dict><key>PATH</key><string>{}</string></dict>
<key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
<key>StandardOutPath</key><string>{}</string><key>StandardErrorPath</key><string>{}</string>
</dict></plist>
"#,
        xml(&executable.to_string_lossy()),
        xml(&path_env),
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
/// PATH for the launchd service. launchd agents get a bare system PATH
/// (`/usr/bin:/bin:/usr/sbin:/sbin`) that hides the tools the daemon shells
/// out to (dropr, tmux, git, the agent CLI). Start from the install-time
/// PATH and ensure the common tool dirs and system dirs are present.
fn service_path_env(current: Option<&str>, home: &std::path::Path) -> String {
    let mut path = current.unwrap_or("").to_string();
    let required = [
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
        home.join(".local/bin").to_string_lossy().into_owned(),
        "/usr/bin".to_string(),
        "/bin".to_string(),
        "/usr/sbin".to_string(),
        "/sbin".to_string(),
    ];
    for dir in required {
        if !path.split(':').any(|entry| entry == dir) {
            if !path.is_empty() {
                path.push(':');
            }
            path.push_str(&dir);
        }
    }
    path
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::{legacy_service_is_absent, service_path_env};
    use std::path::Path;

    #[test]
    fn service_path_env_appends_missing_tool_dirs() {
        let path = service_path_env(
            Some("/usr/bin:/bin:/usr/sbin:/sbin"),
            Path::new("/Users/me"),
        );
        assert_eq!(
            path,
            "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/bin:/Users/me/.local/bin"
        );
    }

    #[test]
    fn service_path_env_keeps_existing_order() {
        let path = service_path_env(
            Some("/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin:/Users/me/.local/bin"),
            Path::new("/Users/me"),
        );
        assert_eq!(
            path,
            "/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin:/Users/me/.local/bin:/usr/local/bin"
        );
    }

    #[test]
    fn service_path_env_builds_from_missing_path() {
        let path = service_path_env(None, Path::new("/Users/me"));
        assert_eq!(
            path,
            "/opt/homebrew/bin:/usr/local/bin:/Users/me/.local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        );
    }

    #[test]
    fn legacy_launchd_cleanup_only_tolerates_absent_service() {
        assert!(legacy_service_is_absent(
            Some(3),
            b"Boot-out failed: 3: No such process\n"
        ));
        assert!(!legacy_service_is_absent(
            Some(5),
            b"Boot-out failed: 5: Input/output error\n"
        ));
        assert!(!legacy_service_is_absent(
            Some(3),
            b"Boot-out failed: 3: Permission denied\n"
        ));
    }
}
