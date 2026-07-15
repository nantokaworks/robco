use std::path::PathBuf;

use crate::Result;

pub mod config;
pub mod ledger;

pub const CHIEF_AGENT_ID: &str = "chief";

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

pub fn decision_log_path() -> Result<PathBuf> {
    Ok(chief_home()?.join("decisions.jsonl"))
}

pub fn heartbeat_path() -> Result<PathBuf> {
    Ok(chief_home()?.join("heartbeat"))
}

pub fn triage_dir() -> Result<PathBuf> {
    Ok(chief_home()?.join("triage"))
}
