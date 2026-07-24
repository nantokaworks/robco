use std::{
    fs,
    path::Path,
    time::{Duration, SystemTime},
};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    config::Config,
    overseer::{heartbeat_path, ledger::Ledger},
};

use super::{ToolResult, exec_err};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PolicyArgs {}

pub(super) fn policy(_args: PolicyArgs) -> ToolResult<Value> {
    let heartbeat = heartbeat_path().map_err(exec_err)?;
    policy_with(
        || Config::load().map_err(exec_err),
        || Ledger::load().map_err(exec_err),
        &heartbeat,
        crate::overseer::daemon_pid_alive(),
    )
}

fn policy_with(
    load_config: impl FnOnce() -> ToolResult<Config>,
    load_ledger: impl FnOnce() -> ToolResult<Ledger>,
    heartbeat: &Path,
    daemon_pid_alive: bool,
) -> ToolResult<Value> {
    let config = load_config()?.overseer;
    let ledger = load_ledger()?;
    let daemon_alive = daemon_pid_alive
        && fs::metadata(heartbeat)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| {
                age <= Duration::from_secs(config.poll_interval_secs.saturating_mul(2).max(5))
            });
    let circuit_open = ledger.counters.consecutive_failures >= config.failure_circuit_threshold;
    Ok(json!({
        "dispatch_enabled": config.dispatch_enabled,
        "auto_merge": config.auto_merge,
        "protection_mode": config.protection_mode.label(),
        "autonomy_level": config.autonomy_level,
        "daily_llm_budget": config.daily_llm_budget,
        // 0 = unlimited (see overseer::dispatch). Surfaced alongside
        // dispatched_today so a zeroed-out limit is visible here rather than only
        // via `robco overseer status`.
        "daily_dispatch_limit": config.daily_dispatch_limit,
        "dispatched_today": ledger.counters.dispatched_today,
        "max_workers": config.max_workers,
        "daemon_alive": daemon_alive,
        "dispatch_without_daemon": config.dispatch_enabled && !daemon_alive,
        "circuit_open": circuit_open,
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
                config.overseer.dispatch_enabled = false;
                config.overseer.max_workers = 2;
                config.overseer.autonomy_level = crate::overseer::autonomy::AutonomyLevel::FullAuto;
                config.overseer.daily_llm_budget = 17;
                Ok(config)
            },
            || Ok(Ledger::default()),
            &heartbeat,
            true,
        )
        .unwrap();
        let second = policy_with(
            || {
                let mut config = Config::default();
                config.overseer.dispatch_enabled = true;
                config.overseer.max_workers = 7;
                Ok(config)
            },
            || Ok(Ledger::default()),
            &heartbeat,
            true,
        )
        .unwrap();
        assert_eq!(first["dispatch_enabled"], false);
        assert_eq!(first["max_workers"], 2);
        assert_eq!(first["autonomy_level"], "full_auto");
        assert_eq!(first["daily_llm_budget"], 17);
        assert_eq!(second["dispatch_enabled"], true);
        assert_eq!(second["max_workers"], 7);
        assert_eq!(second["daemon_alive"], true);
        assert_eq!(second["dispatch_without_daemon"], false);

        let missing_heartbeat = temp.path().join("missing-heartbeat");
        let daemon_down = policy_with(
            || {
                let mut config = Config::default();
                config.overseer.dispatch_enabled = true;
                Ok(config)
            },
            || Ok(Ledger::default()),
            &missing_heartbeat,
            true,
        )
        .unwrap();
        assert_eq!(daemon_down["daemon_alive"], false);
        assert_eq!(daemon_down["dispatch_without_daemon"], true);

        let dead_pid = policy_with(
            || {
                let mut config = Config::default();
                config.overseer.dispatch_enabled = true;
                Ok(config)
            },
            || Ok(Ledger::default()),
            &heartbeat,
            false,
        )
        .unwrap();
        assert_eq!(dead_pid["daemon_alive"], false);
        assert_eq!(dead_pid["dispatch_without_daemon"], true);
    }
}
