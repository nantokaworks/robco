//! `install_service` and the `plist`/`control` submodules are launchd, and
//! launchd is macOS. The consumers in `setup::wizard::steps_service` are
//! gated the same way, so on any other target those items would compile only
//! to sit unused. `ServiceState` and the `*_service` functions below stay
//! platform-agnostic on purpose: `overseer::command`'s `stop`/`start`/
//! `restart` need one signature that works on every target, backed by a
//! macOS body where launchd exists and an `Unsupported`/`unreachable!` one
//! where it does not.

#[cfg(target_os = "macos")]
use std::{fs, process::Command, time::Duration};

#[cfg(target_os = "macos")]
use super::super::{exec::run_timeout, overseer_home};
use crate::Result;

#[cfg(target_os = "macos")]
#[path = "service/plist.rs"]
mod plist;

#[cfg(target_os = "macos")]
#[path = "service/control.rs"]
pub(crate) mod control;

pub(crate) fn install_service() -> Result<()> {
    crate::setup::wizard::steps_service::install_service()
}

/// Whether the launchd Overseer service is installed and, if so, loaded —
/// what `robco overseer stop|start|restart` (dropr:412) branch on to choose
/// between a durable launchd bootout/bootstrap and the manual pidfile path.
/// `Unsupported` stands in for `target_os != "macos"`, where launchd does not
/// exist at all.
///
/// Every match on this enum lives in platform-agnostic code (`command.rs`),
/// but each variant is only ever *constructed* on one side of the
/// `target_os = "macos"` split below — `dead_code` cannot see that the other
/// platform's build supplies the rest, so it is allowed here rather than on
/// each variant.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ServiceState {
    NotInstalled,
    Unloaded,
    Loaded,
    Unsupported,
}

/// See [`ServiceState`]'s doc comment: same platform-split construction, same
/// reason `dead_code` needs the blanket allow rather than per-variant.
#[allow(dead_code)]
pub(crate) enum StopOutcome {
    Stopped,
    StillShuttingDown,
}

#[cfg(target_os = "macos")]
pub(crate) fn service_state() -> ServiceState {
    match control::probe_state() {
        control::ServiceState::NotInstalled => ServiceState::NotInstalled,
        control::ServiceState::Unloaded => ServiceState::Unloaded,
        control::ServiceState::Loaded => ServiceState::Loaded,
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn service_state() -> ServiceState {
    ServiceState::Unsupported
}

/// Durably stop the loaded service. Callers only reach this once
/// [`service_state`] has already reported [`ServiceState::Loaded`].
#[cfg(target_os = "macos")]
pub(crate) fn stop_service() -> Result<StopOutcome> {
    Ok(match control::bootout()? {
        control::StopOutcome::Stopped => StopOutcome::Stopped,
        control::StopOutcome::StillShuttingDown => StopOutcome::StillShuttingDown,
    })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn stop_service() -> Result<StopOutcome> {
    unreachable!(
        "stop_service is only called once service_state() reports Loaded, which never happens off macOS"
    )
}

/// Bootstrap the already-installed plist. Callers only reach this once
/// [`service_state`] has already reported [`ServiceState::Unloaded`].
#[cfg(target_os = "macos")]
pub(crate) fn start_service() -> Result<()> {
    control::bootstrap()
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn start_service() -> Result<()> {
    unreachable!(
        "start_service is only called once service_state() reports Unloaded, which never happens off macOS"
    )
}

/// Bootout then bootstrap the loaded service. Callers only reach this once
/// [`service_state`] has already reported [`ServiceState::Loaded`].
#[cfg(target_os = "macos")]
pub(crate) fn restart_service() -> Result<()> {
    control::bootout()?;
    control::bootstrap()
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn restart_service() -> Result<()> {
    unreachable!(
        "restart_service is only called once service_state() reports Loaded, which never happens off macOS"
    )
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
