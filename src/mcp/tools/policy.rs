use std::{
    fs,
    path::Path,
    time::{Duration, SystemTime},
};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{config::Config, overseer::heartbeat_path};

use super::{ToolResult, exec_err};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PolicyArgs {}

pub(super) fn policy(_args: PolicyArgs) -> ToolResult<Value> {
    let heartbeat = heartbeat_path().map_err(exec_err)?;
    policy_with(
        || Config::load().map_err(exec_err),
        &heartbeat,
        crate::overseer::daemon_pid_alive(),
    )
}

fn policy_with(
    load_config: impl FnOnce() -> ToolResult<Config>,
    heartbeat: &Path,
    daemon_pid_alive: bool,
) -> ToolResult<Value> {
    let config = load_config()?.overseer;
    let daemon_alive = daemon_pid_alive
        && fs::metadata(heartbeat)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| {
                age <= Duration::from_secs(config.poll_interval_secs.saturating_mul(2).max(5))
            });
    Ok(json!({
        "auto_merge": config.auto_merge,
        "protection_mode": config.protection_mode.label(),
        "autonomy_level": config.autonomy_level,
        "daily_llm_budget": config.daily_llm_budget,
        "daemon_alive": daemon_alive,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_uses_config_from_each_call() {
        let temp = tempfile::tempdir().unwrap();
        let heartbeat = temp.path().join("heartbeat");
        fs::write(&heartbeat, "tick").unwrap();
        let first = policy_with(
            || {
                let mut config = Config::default();
                config.overseer.autonomy_level = crate::overseer::autonomy::AutonomyLevel::FullAuto;
                config.overseer.daily_llm_budget = 17;
                Ok(config)
            },
            &heartbeat,
            true,
        )
        .unwrap();
        let second = policy_with(|| Ok(Config::default()), &heartbeat, true).unwrap();
        assert_eq!(first["autonomy_level"], "full_auto");
        assert_eq!(first["daily_llm_budget"], 17);
        assert_eq!(second["daemon_alive"], true);

        let missing_heartbeat = temp.path().join("missing-heartbeat");
        let daemon_down = policy_with(|| Ok(Config::default()), &missing_heartbeat, true).unwrap();
        assert_eq!(daemon_down["daemon_alive"], false);

        let dead_pid = policy_with(|| Ok(Config::default()), &heartbeat, false).unwrap();
        assert_eq!(dead_pid["daemon_alive"], false);
    }
}
