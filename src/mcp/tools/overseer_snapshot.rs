use std::{
    fs,
    time::{Duration, SystemTime},
};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    config::Config,
    model::Status,
    overseer::{
        discord_channels::DiscordChannels, dismissals::Dismissals, ledger::Ledger, logging,
        other_prs::OtherPrs, row_summaries::RowSummaries,
    },
    status::{self, WatchStatusState},
};

use super::{ToolResult, exec_err};

const DECISION_SNAPSHOT_LIMIT: usize = 200;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OverseerSnapshotArgs {}

pub(super) fn snapshot(_args: OverseerSnapshotArgs) -> ToolResult<Value> {
    let config = Config::load().map_err(exec_err)?;
    let heartbeat = crate::overseer::heartbeat_path().ok();
    let heartbeat_age = heartbeat
        .as_ref()
        .and_then(|path| fs::metadata(path).and_then(|meta| meta.modified()).ok())
        .and_then(|modified| SystemTime::now().duration_since(modified).ok());
    let daemon_pid_alive = crate::overseer::daemon_pid_alive();
    let freshness =
        Duration::from_secs(config.overseer.poll_interval_secs.saturating_mul(2).max(5));
    let daemon_alive = daemon_pid_alive && heartbeat_age.is_some_and(|age| age <= freshness);
    let daemon_version = heartbeat
        .as_ref()
        .and_then(|path| crate::overseer::heartbeat::recorded_version(path));
    let control_session = crate::overseer::control_session_name(&config.tmux_session_prefix);
    let control_status =
        status::classify_session_status(&control_session, None, &mut WatchStatusState::default())
            .map(status_label);
    let discord_channels = crate::overseer::discord_ops_dir()
        .ok()
        .and_then(|dir| DiscordChannels::load(&dir.join("channels.json")).ok())
        .unwrap_or_default();

    Ok(json!({
        "overseer": config.overseer,
        "ledger": Ledger::load().unwrap_or_default(),
        "other_prs": OtherPrs::load().unwrap_or_default(),
        "discord_channels": discord_channels,
        "decisions": logging::tail(DECISION_SNAPSHOT_LIMIT).unwrap_or_default(),
        "dismissals": Dismissals::load().unwrap_or_default(),
        "row_summaries": RowSummaries::load().unwrap_or_default(),
        "daemon_pid_alive": daemon_pid_alive,
        "daemon_alive": daemon_alive,
        "heartbeat_age": heartbeat_age,
        "daemon_version": daemon_version,
        "control_session": control_session,
        "control_status": control_status,
    }))
}

fn status_label(status: Status) -> &'static str {
    status.badge()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_status_uses_stable_badge_names() {
        assert_eq!(status_label(Status::Running), "run");
        assert_eq!(status_label(Status::Waiting), "wait");
        assert_eq!(status_label(Status::Done), "done");
    }
}
