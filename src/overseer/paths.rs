use std::path::PathBuf;

use crate::Result;

use super::{exec, overseer_home};

pub fn ledger_path() -> Result<PathBuf> {
    Ok(overseer_home()?.join("ledger.json"))
}

pub fn runtime_requests_dir() -> Result<PathBuf> {
    Ok(overseer_home()?.join("runtime_requests"))
}

pub fn inbox_path() -> Result<PathBuf> {
    Ok(overseer_home()?.join("inbox.jsonl"))
}

/// Inbox rows the operator has cleared. Suppression only — the decisions and
/// ledger entries the rows are derived from are never touched.
pub fn inbox_dismissals_path() -> Result<PathBuf> {
    Ok(overseer_home()?.join("inbox_dismissals.json"))
}

pub fn pidfile_path() -> Result<PathBuf> {
    Ok(overseer_home()?.join("overseer.pid"))
}

/// True when the Overseer daemon pidfile names a live process. Combined with a
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
    Ok(overseer_home()?.join("decisions.jsonl"))
}

pub fn discord_cursor_path() -> Result<PathBuf> {
    Ok(overseer_home()?.join("discord.cursor"))
}

pub fn heartbeat_path() -> Result<PathBuf> {
    Ok(overseer_home()?.join("heartbeat"))
}

pub fn snapshots_path() -> Result<PathBuf> {
    Ok(overseer_home()?.join("observations.jsonl"))
}

/// Last auto-merge pass's duration and slowest repository — see
/// `daemon::merge_pass_telemetry`.
pub fn merge_pass_path() -> Result<PathBuf> {
    Ok(overseer_home()?.join("merge_pass.json"))
}

/// Pull requests discovered in a watched repository that Overseer did not
/// dispatch — see [`super::other_prs`]. Its own file, deliberately apart from
/// `ledger.json`: the ledger records only what Overseer itself dispatched.
pub fn other_prs_path() -> Result<PathBuf> {
    Ok(overseer_home()?.join("other_prs.json"))
}

/// When each managed repository's periodic advisory/Dependabot health watch
/// last ran — see [`super::repo_watch`]. Its own file for the same reason
/// `other_prs_path` gets one: this cadence cache is not the ledger.
pub fn repo_watch_path() -> Result<PathBuf> {
    Ok(overseer_home()?.join("repo_watch.json"))
}

/// Whether the dispatch pass's ready-candidate gather found the board
/// already drained as of its last recorded pass. Persisted so a daemon
/// restart neither re-announces a drain that already fired nor announces
/// one that never happened — see `crate::overseer::dispatch::drain`.
pub fn queue_drained_state_path() -> Result<PathBuf> {
    Ok(overseer_home()?.join("queue_drained.json"))
}

pub fn triage_dir() -> Result<PathBuf> {
    Ok(overseer_home()?.join("triage"))
}

pub fn judge_dir() -> Result<PathBuf> {
    Ok(overseer_home()?.join("judge"))
}

/// Case directory for the start-up credential probe. Its own directory rather
/// than a judge case so a probe never looks like a judgment in the retained
/// case history.
pub fn preflight_dir() -> Result<PathBuf> {
    Ok(overseer_home()?.join("preflight"))
}

/// Last verdict on whether a daemon-spawned session can authenticate. Written
/// by the start-up probe and by any session refused on credentials; read by
/// `robco overseer status`.
pub fn session_health_path() -> Result<PathBuf> {
    Ok(overseer_home()?.join("session_health.json"))
}

pub fn review_dir() -> Result<PathBuf> {
    Ok(overseer_home()?.join("review"))
}

pub fn discord_ops_dir() -> Result<PathBuf> {
    Ok(overseer_home()?.join("discord-ops"))
}
