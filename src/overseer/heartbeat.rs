//! The Overseer daemon's heartbeat file, and the build that writes it.
//!
//! Every pass rewrites this file, so its modification time is the liveness
//! signal every status surface reads. The daemon keeps running the image it
//! started from until the service restarts, so the file also records that
//! build's version: without it, a fix that is merged, released, and installed
//! is indistinguishable — on every surface — from one the running daemon
//! already carries, and `daemon: healthy` reads as "up to date" when it only
//! means "up".

use std::{fs, path::Path};

use chrono::{DateTime, Utc};
use nanoid::nanoid;

use crate::Result;

/// The build answering the current command.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Terse header label for a daemon whose build is not this one. The full
/// sentence — which names both versions — is [`drift`]; this is what fits
/// beside the OVERSEER label on a 24-column sidebar.
pub const DRIFT_LABEL: &str = "stale build";

/// Introduces the version line. The timestamp keeps the first line to itself,
/// exactly as builds before this one wrote it.
const VERSION_PREFIX: &str = "version=";

/// The one recovery, named in both drift messages: the daemon rereads its
/// executable only at startup, so nothing short of a restart moves it.
const RESTART_HINT: &str = "restart it with `robco stop` and `robco daemon` (or restart the installed service) to pick up the installed build";

/// Record a completed pass, with the build that ran it.
///
/// Written through a temp file and renamed, like every other state file the
/// daemon owns. The timestamp alone tolerated a torn read because only the
/// modification time was ever consulted; a reader that catches a truncated
/// version field would report a build drift that does not exist, which is the
/// exact false signal this file exists to remove.
pub fn write(path: &Path, at: DateTime<Utc>) -> Result<()> {
    let temp_path = path.with_extension(format!("{}.tmp", nanoid!()));
    let written = fs::write(&temp_path, render(at)).and_then(|()| fs::rename(&temp_path, path));
    if let Err(error) = written {
        let _ = fs::remove_file(temp_path);
        return Err(error.into());
    }
    Ok(())
}

fn render(at: DateTime<Utc>) -> String {
    format!("{}\n{VERSION_PREFIX}{VERSION}\n", at.to_rfc3339())
}

/// The build the daemon that last wrote `path` started from.
///
/// `None` covers both the missing file and a heartbeat written before this
/// field existed; [`drift`] is what decides what that absence means.
pub fn recorded_version(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    raw.lines()
        .find_map(|line| line.trim().strip_prefix(VERSION_PREFIX))
        .map(str::trim)
        .filter(|version| !version.is_empty())
        .map(str::to_owned)
}

/// How the running daemon's build differs from this binary's, or `None` when
/// the two match and there is nothing to act on.
///
/// An absent version is drift rather than an unknown: only a build older than
/// the one that started recording it omits the field, and that build is by
/// definition not this one. Callers gate this on the daemon actually being
/// healthy — telling an operator to restart a daemon that is already reported
/// down names a recovery they are being pointed at anyway.
pub fn drift(recorded: Option<&str>) -> Option<String> {
    match recorded {
        Some(version) if version == VERSION => None,
        Some(version) => Some(format!(
            "overseer daemon is running {version} but this binary is {VERSION} — anything fixed since {version} is not live yet; {RESTART_HINT}"
        )),
        None => Some(format!(
            "overseer daemon records no build, so it started from a release older than {VERSION} — anything fixed since is not live yet; {RESTART_HINT}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_version_reads_back_what_the_daemon_wrote() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("heartbeat");
        write(&path, Utc::now()).unwrap();

        assert_eq!(recorded_version(&path).as_deref(), Some(VERSION));
        // The rename leaves nothing behind for the next reader to trip over.
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
        // The timestamp stays on the first line, where every reader before this
        // change expected to find it.
        let raw = fs::read_to_string(&path).unwrap();
        assert!(
            DateTime::parse_from_rfc3339(raw.lines().next().unwrap()).is_ok(),
            "first heartbeat line is not an rfc3339 timestamp: {raw:?}"
        );
    }

    #[test]
    fn a_heartbeat_from_before_version_recording_has_no_version() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("heartbeat");
        fs::write(&path, Utc::now().to_rfc3339()).unwrap();

        assert_eq!(recorded_version(&path), None);
        assert_eq!(recorded_version(&temp.path().join("missing")), None);
    }

    #[test]
    fn an_empty_version_reads_as_absent() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("heartbeat");
        fs::write(&path, format!("{}\nversion=\n", Utc::now().to_rfc3339())).unwrap();

        assert_eq!(recorded_version(&path), None);
    }

    #[test]
    fn a_daemon_on_this_build_does_not_drift() {
        assert_eq!(drift(Some(VERSION)), None);
    }

    #[test]
    fn drift_names_both_builds() {
        let warning = drift(Some("0.0.1")).expect("differing builds must warn");
        assert!(warning.contains("0.0.1"), "{warning}");
        assert!(warning.contains(VERSION), "{warning}");
        assert!(warning.contains("robco stop"), "{warning}");
    }

    #[test]
    fn an_absent_build_warns_without_naming_a_running_version() {
        let warning = drift(None).expect("an unrecorded build predates this one");
        assert!(warning.contains(VERSION), "{warning}");
        assert!(warning.contains("older"), "{warning}");
    }
}
