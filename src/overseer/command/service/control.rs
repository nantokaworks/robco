//! Runtime control for the *installed* launchd Overseer service: durable
//! stop and start, for `robco overseer stop|start|restart` (dropr:412).
//!
//! Built on the same probe/settle primitives the setup wizard's install and
//! reload flow already relies on (`crate::setup::wizard::steps_service`),
//! rather than re-deriving the same bootout-wait / bootstrap-retry dance: a
//! `KeepAlive` job only leaves the domain through `launchctl bootout` — a
//! bare `SIGTERM` gets it respawned — and `bootout` is asynchronous, so the
//! label has to be observed leaving the domain before anything reloads.

use std::{ffi::OsString, path::PathBuf, process::Command, time::Duration};

use crate::{
    Error, Result,
    overseer::exec::run_timeout,
    setup::wizard::steps_service::{
        probe,
        settle::{Domain, SettleBudget},
    },
};

const LABEL: &str = "com.robco.overseer";

pub(crate) use probe::ServiceState;

pub(crate) enum StopOutcome {
    Stopped,
    StillShuttingDown,
}

pub(crate) fn probe_state() -> ServiceState {
    probe::run().state
}

/// `launchctl bootout`, then wait for the label to actually leave the
/// domain — see the module doc for why a bare signal is not durable here.
pub(crate) fn bootout() -> Result<StopOutcome> {
    let domain_name = gui_domain()?;
    let service = format!("{domain_name}/{LABEL}");
    let mut command = Command::new("launchctl");
    command.args(["bootout", &service]);
    let output = run_timeout(command, Duration::from_secs(5))?;
    if !output.status.success() && !service_is_absent(output.status.code(), &output.stderr) {
        return Err(Error::Command {
            context: "stop launchd overseer service",
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    let domain = Domain {
        name: &domain_name,
        plist: &plist_path()?,
        budget: SettleBudget::launchd(),
    };
    Ok(match domain.wait_for_bootout(&mut run_launchctl) {
        Ok(()) => StopOutcome::Stopped,
        Err(_) => StopOutcome::StillShuttingDown,
    })
}

/// `launchctl bootstrap` the already-installed plist, retrying while launchd
/// reports the domain busy, then verify the service actually came up.
pub(crate) fn bootstrap() -> Result<()> {
    let domain_name = gui_domain()?;
    let domain = Domain {
        name: &domain_name,
        plist: &plist_path()?,
        budget: SettleBudget::launchd(),
    };
    domain.bootstrap(&mut run_launchctl)?;
    if probe_state() != ServiceState::Loaded {
        return Err(Error::Command {
            context: "start launchd overseer service",
            stderr: "service did not come up after bootstrap".into(),
        });
    }
    Ok(())
}

fn run_launchctl(args: &[OsString]) -> std::io::Result<std::process::Output> {
    let mut command = Command::new("launchctl");
    command.args(args);
    run_timeout(command, Duration::from_secs(5))
}

fn gui_domain() -> Result<String> {
    let mut id = Command::new("id");
    id.arg("-u");
    let output = run_timeout(id, Duration::from_secs(2))?;
    if !output.status.success() {
        return Err(Error::Command {
            context: "look up user id for launchd overseer control",
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    let uid = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok(format!("gui/{uid}"))
}

fn plist_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or(Error::HomeDir)?;
    Ok(home
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist")))
}

fn service_is_absent(status: Option<i32>, stderr: &[u8]) -> bool {
    status == Some(3) && String::from_utf8_lossy(stderr).contains("No such process")
}
