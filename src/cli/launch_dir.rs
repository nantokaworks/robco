//! Validation for the positional launch-directory arguments.
//!
//! `robco odin` used to open an empty TUI when `odin` was not a directory:
//! discovery skips an unreadable root without a word, so a typo — or a host
//! name typed where a directory belongs — read as "connected, nothing here"
//! (dropr:586). Failing fast, with a hint at the remote-host path the
//! operator probably wanted, keeps the mistake visible. The check runs
//! before any state is touched, so a refused launch creates nothing.

use std::path::Path;

use crate::{Error, Result};

/// Errors unless `path` (when given) names an existing directory.
///
/// `None` is fine — an omitted launch directory falls back to the configured
/// `repos_root`, which discovery has always been allowed to find empty.
pub(crate) fn validate_optional(path: Option<&Path>) -> Result<()> {
    match path {
        Some(path) if !path.is_dir() => Err(Error::LaunchDirMissing(path.to_path_buf())),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_existing_directory_passes() {
        let dir = tempfile::tempdir().unwrap();
        assert!(validate_optional(Some(dir.path())).is_ok());
    }

    #[test]
    fn an_omitted_directory_passes() {
        assert!(validate_optional(None).is_ok());
    }

    #[test]
    fn a_bare_word_that_is_no_directory_errors_with_the_remote_hint() {
        let error = validate_optional(Some(Path::new("odin")))
            .unwrap_err()
            .to_string();
        assert!(error.contains("'odin' is not a directory"), "{error}");
        assert!(error.contains("--host <destination>"), "{error}");
        assert!(error.contains("H key"), "{error}");
    }

    #[test]
    fn a_missing_relative_path_errors() {
        let error = validate_optional(Some(Path::new("./missing-dir")))
            .unwrap_err()
            .to_string();
        assert!(error.contains("is not a directory"), "{error}");
    }

    #[test]
    fn a_file_is_not_a_directory() {
        let file = tempfile::NamedTempFile::new().unwrap();
        assert!(validate_optional(Some(file.path())).is_err());
    }
}
