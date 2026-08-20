//! Operator commands that persist a setting in `~/.robco/config.json`.
//!
//! Each of these reloads the config rather than mutating the caller's snapshot: the
//! daemon rewrites the file on its own ticks, so a load→mutate→save keeps a CLI write
//! from clobbering a concurrent one with a stale snapshot.

use std::path::Path;

use super::on_off;
use crate::{Result, cli::OverseerSetting, config::Config, overseer::config::ProtectionMode};

pub(super) fn set(setting: OverseerSetting, enabled: bool) -> Result<()> {
    set_runtime(setting, enabled)?;
    let label = match setting {
        OverseerSetting::AutoMerge => "auto-merge",
    };
    println!("{label}: {}", on_off(enabled));
    Ok(())
}

pub(crate) fn set_runtime(setting: OverseerSetting, enabled: bool) -> Result<()> {
    let mut config = Config::load()?;
    match setting {
        OverseerSetting::AutoMerge => {
            config.overseer.auto_merge = enabled;
        }
    }
    config.save()?;
    Ok(())
}

/// `robco config notify-channel <id>` / `--clear`: where reports — decision
/// notifications and digests — are delivered. The id is validated here so a
/// typo fails at the terminal instead of silently falling back to the chat
/// channel on every send (see `notify::report_channel_id`).
pub(super) fn notify_channel(value: Option<String>) -> Result<()> {
    if let Some(raw) = value.as_deref()
        && raw.parse::<u64>().ok().filter(|id| *id != 0).is_none()
    {
        return Err(std::io::Error::other(format!(
            "invalid Discord channel id: {raw} (expected a non-zero number)"
        ))
        .into());
    }
    crate::config::ensure_robco_dir()?;
    persist_notify_channel_at(&crate::config::config_file_path()?, value.clone())?;
    match value {
        Some(id) => println!("notify channel: {id}"),
        None => println!("notify channel: unset (reports follow the chat channel)"),
    }
    Ok(())
}

/// Split from the printing so the round-trip can be tested against a temp file
/// rather than the operator's own `~/.robco/config.json`.
fn persist_notify_channel_at(path: &Path, value: Option<String>) -> Result<()> {
    let mut config = Config::load_at(path)?;
    config.overseer.discord.notify_channel_id = value;
    config.save_at(path)
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

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
