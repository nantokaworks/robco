//! Everything below `install_service` is launchd, and launchd is macOS. The
//! consumers in `setup::wizard::steps_service` are gated the same way, so on
//! any other target these items would compile only to sit unused.

#[cfg(target_os = "macos")]
use std::{fs, process::Command, time::Duration};

#[cfg(target_os = "macos")]
use super::super::{exec::run_timeout, overseer_home};
use crate::Result;

#[cfg(target_os = "macos")]
#[path = "service/plist.rs"]
mod plist;

pub(crate) fn install_service() -> Result<()> {
    crate::setup::wizard::steps_service::install_service()
}

#[cfg(target_os = "macos")]
fn remove_legacy_service() -> Result<()> {
    let home = dirs::home_dir().ok_or(crate::Error::HomeDir)?;
    let legacy_plist = home.join("Library/LaunchAgents/com.robco.chief.plist");
    let mut id = Command::new("id");
    id.arg("-u");
    let output = run_timeout(id, Duration::from_secs(2))?;
    if !output.status.success() {
        return Err(crate::Error::Command {
            context: "look up user id for legacy launchd cleanup",
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    let uid = String::from_utf8_lossy(&output.stdout);
    let uid = uid.trim().parse::<u32>().map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid user id returned by `id -u`: {error}"),
        )
    })?;
    let mut command = Command::new("launchctl");
    command.args(["bootout", &format!("gui/{uid}/com.robco.chief")]);
    let output = run_timeout(command, Duration::from_secs(5))?;
    if !output.status.success() && !legacy_service_is_absent(output.status.code(), &output.stderr) {
        return Err(crate::Error::Command {
            context: "boot out legacy launchd service",
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    if legacy_plist.try_exists()? {
        fs::remove_file(legacy_plist)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn legacy_service_is_absent(status: Option<i32>, stderr: &[u8]) -> bool {
    status == Some(3) && String::from_utf8_lossy(stderr).contains("No such process")
}

/// Write the launchd job definition, carrying the configured session
/// environment so the daemon's sessions have a credential to run under.
///
/// The config is loaded here rather than threaded in from the wizard: this is
/// the installer, and what it installs is a function of the config on disk. A
/// config that cannot be read installs the defaults, which is the same daemon
/// the operator would get by running `robco overseer run` by hand.
#[cfg(target_os = "macos")]
pub(crate) fn write_service_plist() -> Result<std::path::PathBuf> {
    remove_legacy_service()?;
    let home = dirs::home_dir().ok_or(crate::Error::HomeDir)?;
    let dir = home.join("Library/LaunchAgents");
    fs::create_dir_all(&dir)?;
    let path = dir.join("com.robco.overseer.plist");
    let config = crate::config::Config::load().unwrap_or_default();
    let body = plist::render(
        &std::env::current_exe()?,
        &plist::path_env(std::env::var("PATH").ok().as_deref(), &home),
        &overseer_home()?.join("overseer.log"),
        &config.overseer.session_env,
    );
    fs::write(&path, body)?;
    // The dictionary can now hold a credential, so the file is no longer
    // world-readable metadata. Tightened after the write rather than before, so
    // a re-install narrows a plist an earlier version left at its umask default.
    fs::set_permissions(
        &path,
        std::os::unix::fs::PermissionsExt::from_mode(plist::PLIST_MODE),
    )?;
    Ok(path)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::legacy_service_is_absent;

    #[test]
    fn legacy_launchd_cleanup_only_tolerates_absent_service() {
        assert!(legacy_service_is_absent(
            Some(3),
            b"Boot-out failed: 3: No such process\n"
        ));
        assert!(!legacy_service_is_absent(
            Some(5),
            b"Boot-out failed: 5: Input/output error\n"
        ));
        assert!(!legacy_service_is_absent(
            Some(3),
            b"Boot-out failed: 3: Permission denied\n"
        ));
    }
}
