use std::{
    fs,
    path::Path,
    time::{Duration, SystemTime},
};

#[cfg(test)]
use chrono::Utc;
use ratatui::text::Text;

#[cfg(test)]
use crate::overseer::config::OverseerConfig;
use crate::{
    config::Config,
    model::Status,
    overseer::{heartbeat_path, ledger::Ledger},
};

use super::App;

mod render;

use render::{append_decisions, append_health, append_inbox, append_ledger, detail, warning};

pub(super) fn summary(app: &App) -> (String, Text<'static>) {
    let config = Config::load().map(|config| config.overseer);
    let ledger = Ledger::load();
    let heartbeat = heartbeat_path();
    let mut lines = Vec::new();

    match (config, ledger, heartbeat) {
        (Ok(config), Ok(ledger), Ok(heartbeat)) => {
            append_health(&mut lines, &config, &ledger, &heartbeat);
            append_ledger(&mut lines, &config, &ledger);
            append_inbox(&mut lines, app);
            append_decisions(&mut lines);
        }
        (config, ledger, heartbeat) => {
            lines.push(warning("Overseer state could not be read."));
            if let Err(error) = config {
                lines.push(detail("config", error));
            }
            if let Err(error) = ledger {
                lines.push(detail("ledger", error));
            }
            if let Err(error) = heartbeat {
                lines.push(detail("heartbeat", error));
            }
        }
    }
    ("OVERSEER / local control".into(), lines.into())
}

pub(super) fn status() -> Status {
    let config = Config::load()
        .map(|config| config.overseer)
        .unwrap_or_default();
    if crate::overseer::daemon_pid_alive()
        && heartbeat_path()
            .ok()
            .is_some_and(|path| heartbeat_is_fresh(&path, config.poll_interval_secs))
    {
        Status::Running
    } else {
        Status::Dead
    }
}

pub(super) fn heartbeat_is_fresh(path: &Path, poll_secs: u64) -> bool {
    heartbeat_is_fresh_at(path, poll_secs, SystemTime::now())
}

fn heartbeat_is_fresh_at(path: &Path, poll_secs: u64, now: SystemTime) -> bool {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| now.duration_since(modified).ok())
        .is_some_and(|age| age <= heartbeat_limit(poll_secs))
}

fn heartbeat_limit(poll_secs: u64) -> Duration {
    Duration::from_secs(poll_secs.saturating_mul(2).max(5))
}

#[cfg(test)]
mod tests;
