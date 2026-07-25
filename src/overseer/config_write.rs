//! The daemon's own writes to `~/.robco/config.json`.
//!
//! A pass reloads the config at its top but writes back near the end, after passes
//! that block on model sessions — minutes can separate the two. Serialising the
//! pass-start snapshot would revert whatever an operator edited in that window, and
//! silently, since nothing logs the overwrite. So the write reloads the file and
//! touches only the single field the daemon owns, the same load→mutate→save
//! discipline `command::settings` uses for CLI writes.

use std::path::Path;

use crate::{
    Result,
    config::{Config, config_file_path},
};

/// Persist `overseer.dispatch_enabled`, the one config field the daemon writes.
/// Returns whether the file was rewritten so the caller can log the overwrite;
/// a value already on disk needs no write and reports `false`.
pub(crate) fn persist_dispatch_enabled(enabled: bool) -> Result<bool> {
    persist_dispatch_enabled_at(&config_file_path()?, enabled)
}

pub(crate) fn persist_dispatch_enabled_at(path: &Path, enabled: bool) -> Result<bool> {
    let mut config = Config::load_at(path)?;
    if config.overseer.dispatch_enabled == enabled {
        return Ok(false);
    }
    config.overseer.dispatch_enabled = enabled;
    config.save_at(path)?;
    Ok(true)
}

#[cfg(test)]
#[path = "config_write_tests.rs"]
mod tests;
