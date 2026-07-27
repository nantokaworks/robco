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

#[cfg(not(test))]
pub(crate) fn robco_dir() -> Result<PathBuf> {
    let home = home_dir().ok_or(crate::Error::HomeDir)?;
    Ok(home.join(".robco"))
}

#[cfg(not(test))]
pub(crate) fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

/// Every path in this module (state, config, ui-state, and everything under
/// `overseer_home()`) is built on `robco_dir()`/`home_dir()`, so redirecting
/// these two functions for the whole test binary is enough to keep the entire
/// suite off the operator's real `~/.robco` — a new test cannot opt out of the
/// redirect because it never resolves the real home to begin with.
#[cfg(test)]
pub(crate) fn robco_dir() -> Result<PathBuf> {
    Ok(test_home().join(".robco"))
}

#[cfg(test)]
pub(crate) fn home_dir() -> Option<PathBuf> {
    Some(test_home())
}

/// One throwaway home directory per test process, created on first use and
/// shared by every test that runs after it (tests share a process and its
/// environment, so a per-call tempdir would still let concurrent tests race
/// on a freshly resolved but different fake home each time).
#[cfg(test)]
fn test_home() -> PathBuf {
    use std::sync::OnceLock;
    static TEST_HOME: OnceLock<tempfile::TempDir> = OnceLock::new();
    TEST_HOME
        .get_or_init(|| tempfile::tempdir().expect("create isolated test home"))
        .path()
        .to_path_buf()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Guards the isolation invariant itself, not just today's callers: if a
    /// future refactor ever lets `robco_dir()`/`home_dir()` fall through to the
    /// real home during `cargo test`, this fails the suite instead of quietly
    /// resuming writes into the operator's live state.
    #[test]
    fn robco_dir_and_home_dir_never_resolve_to_the_real_home_during_tests() {
        let fake_home = home_dir().expect("fake test home must resolve");
        if let Some(real_home) = dirs::home_dir() {
            assert_ne!(
                fake_home, real_home,
                "config::home_dir() resolved to the operator's real home during tests"
            );
        }
        assert!(
            robco_dir().unwrap().starts_with(&fake_home),
            "robco_dir() escaped the isolated test home"
        );
    }

    #[test]
    fn test_home_is_stable_within_one_process() {
        assert_eq!(test_home(), test_home());
    }
}
