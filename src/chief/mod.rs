use std::path::PathBuf;

use crate::Result;

pub mod command;
pub mod config;
pub mod daemon;
pub mod discord;
pub mod dispatch;
pub mod exec;
pub mod inbox;
pub mod ledger;
pub mod logging;
pub mod monitor;
pub(crate) mod session;
pub mod templates;
pub mod triage;

pub const CHIEF_AGENT_ID: &str = "chief";

/// Shown when dispatch is enabled but the Chief daemon is not running: the
/// toggle is on yet no poll loop consumes ready tasks, so name the two
/// supported ways to start the daemon.
pub const DISPATCH_WITHOUT_DAEMON_HINT: &str = "dispatch is on but the Chief daemon is not running — no tasks will be dispatched. Start it with `robco chief run`, or install the always-on service with `robco chief install-service`.";

pub fn chief_home() -> Result<PathBuf> {
    Ok(crate::config::robco_dir()?.join(CHIEF_AGENT_ID))
}

pub fn ledger_path() -> Result<PathBuf> {
    Ok(chief_home()?.join("ledger.json"))
}

pub fn inbox_path() -> Result<PathBuf> {
    Ok(chief_home()?.join("inbox.jsonl"))
}

pub fn pidfile_path() -> Result<PathBuf> {
    Ok(chief_home()?.join("chief.pid"))
}

/// True when the Chief daemon pidfile names a live process. Combined with a
/// fresh heartbeat this is the canonical "daemon is running" signal shared by
/// every status surface (CLI, MCP policy, TUI).
pub fn daemon_pid_alive() -> bool {
    pidfile_path()
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .is_some_and(exec::process_alive)
}

pub fn decision_log_path() -> Result<PathBuf> {
    Ok(chief_home()?.join("decisions.jsonl"))
}

pub fn discord_cursor_path() -> Result<PathBuf> {
    Ok(chief_home()?.join("discord.cursor"))
}

pub fn heartbeat_path() -> Result<PathBuf> {
    Ok(chief_home()?.join("heartbeat"))
}

pub fn snapshots_path() -> Result<PathBuf> {
    Ok(chief_home()?.join("observations.jsonl"))
}

pub fn triage_dir() -> Result<PathBuf> {
    Ok(chief_home()?.join("triage"))
}

pub fn discord_ops_dir() -> Result<PathBuf> {
    Ok(chief_home()?.join("discord-ops"))
}
