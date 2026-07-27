//! Locations under `~/.robco` and the tilde expansion applied to configured paths.

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::Result;

pub fn state_path() -> Result<PathBuf> {
    Ok(robco_dir()?.join("state.json"))
}

pub fn ensure_robco_dir() -> Result<PathBuf> {
    let dir = robco_dir()?;
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub(crate) fn config_path() -> Result<PathBuf> {
    Ok(robco_dir()?.join("config.json"))
}

/// Home of the sidebar layout the operator arranged. Deliberately its own file:
/// `state.json` is rewritten by every discovery refresh and `config.json` holds
/// settings the operator edits by hand, so neither can own UI state.
pub(crate) fn ui_state_path() -> Result<PathBuf> {
    Ok(robco_dir()?.join("ui-state.json"))
}

pub fn config_file_path() -> Result<PathBuf> {
    config_path()
}

pub(crate) fn robco_dir() -> Result<PathBuf> {
    let home = home_dir().ok_or(crate::Error::HomeDir)?;
    Ok(home.join(".robco"))
}

pub(crate) fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

/// Expand a leading `~` component to the home directory. Paths without a `~`
/// prefix, and paths that cannot be expanded (no home dir), are returned as-is.
pub(crate) fn expand_tilde(path: &Path) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(rest) => match home_dir() {
            Some(home) => home.join(rest),
            None => path.to_path_buf(),
        },
        Err(_) => path.to_path_buf(),
    }
}
