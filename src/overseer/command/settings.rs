//! Operator commands that persist a setting in `~/.robco/config.json`.
//!
//! Each of these reloads the config rather than mutating the caller's snapshot: the
//! daemon rewrites the file on its own ticks, so a load→mutate→save keeps a CLI write
//! from clobbering a concurrent one with a stale snapshot.

use super::{daemon_healthy, on_off};
use crate::{
    Result,
    cli::OverseerSetting,
    config::Config,
    overseer::{
        config::ProtectionMode,
        runtime_request::{self, RuntimeRequest},
    },
};

pub(super) fn set(config: &Config, setting: OverseerSetting, enabled: bool) -> Result<()> {
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
        runtime_request::enqueue(RuntimeRequest::ResetCircuit {
            source: "cli".into(),
            at: chrono::Utc::now(),
        })?;
    }
    Ok(())
}

pub(super) fn daily_limit(value: u32) -> Result<()> {
    let mut config = Config::load()?;
    config.overseer.daily_dispatch_limit = value;
    config.save()?;
    println!(
        "daily dispatch limit: {}",
        crate::overseer::dispatch::format_dispatch_limit(value)
    );
    Ok(())
}

pub(super) fn protection_mode(mode: ProtectionMode) -> Result<()> {
    let mut config = Config::load()?;
    config.overseer.protection_mode = mode;
    config.save()?;
    println!("protection: {}", mode.label());
    if let Some(warning) = protection_warning(mode) {
        println!("warning: {warning}");
    }
    Ok(())
}

/// Names what a loosened gate no longer verifies, so `status` and the mode command warn
/// in the same words. `None` for the default mode, which verifies everything.
pub(super) fn protection_warning(mode: ProtectionMode) -> Option<&'static str> {
    match mode {
        ProtectionMode::Required => None,
        ProtectionMode::Relaxed => {
            Some("auto-merge no longer requires status-check protection on the base branch")
        }
        ProtectionMode::Off => Some("auto-merge no longer verifies base-branch protection at all"),
    }
}
