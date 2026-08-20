//! Runtime control for the *installed* launchd Overseer service: durable
//! stop and start, for `robco stop|start|restart` (dropr:412).
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
    overseer::{
        daemon_pid,
        exec::{process_alive, run_timeout},
    },
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
/// domain *and* for the process the pidfile named just before teardown to
/// actually exit — see the module doc for why a bare signal is not durable
/// here, and [`wait_for_pid_exit`] for why the label alone is not enough
/// (dropr:483).
pub(crate) fn bootout() -> Result<StopOutcome> {
    let domain_name = gui_domain()?;
    let service = format!("{domain_name}/{LABEL}");
    let old_pid = daemon_pid();
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
    let label_gone = domain.wait_for_bootout(&mut run_launchctl).is_ok();
    let process_gone = wait_for_pid_exit(old_pid, SettleBudget::launchd());
    Ok(if label_gone && process_gone {
        StopOutcome::Stopped
    } else {
        StopOutcome::StillShuttingDown
    })
}

/// Poll until `pid` (the pidfile's occupant just before teardown) is no
/// longer alive, giving up after `budget.polls` attempts. `None` — no
/// pidfile at teardown time — counts as already gone.
///
/// `wait_for_bootout` only proves the launchd label left the domain; the
/// daemon is `KeepAlive` and can still be mid-exit behind it, and the
/// pidfile guard (`PidGuard::acquire`) then makes a new process racing that
/// exit lose silently — the old binary keeps running under a label that
/// already looks free (dropr:483).
fn wait_for_pid_exit(pid: Option<u32>, budget: SettleBudget) -> bool {
    wait_for_pid_exit_with(pid, budget, process_alive)
}

/// [`wait_for_pid_exit`] split from its real `kill -0` liveness check so the
/// poll/give-up shape is testable without spawning and reaping real
/// processes — see `control_tests.rs`.
fn wait_for_pid_exit_with(
    pid: Option<u32>,
    budget: SettleBudget,
    mut alive: impl FnMut(u32) -> bool,
) -> bool {
    let Some(pid) = pid else { return true };
    for attempt in 0..budget.polls {
        if attempt > 0 {
            std::thread::sleep(budget.poll_interval);
        }
        if !alive(pid) {
            return true;
        }
    }
    false
}

/// Bootout then bootstrap the loaded service, refusing to bootstrap onto a
/// daemon that has not actually exited and confirming a new one took its
/// place afterward — so `robco restart` never reports success
/// while the daemon it started with is still the one running (dropr:483).
pub(crate) fn restart() -> Result<()> {
    let old_pid = daemon_pid();
    match bootout()? {
        StopOutcome::Stopped => {}
        StopOutcome::StillShuttingDown => return Err(still_shutting_down_error(old_pid)),
    }
    bootstrap()?;
    verify_new_process(old_pid)
}

fn still_shutting_down_error(pid: Option<u32>) -> Error {
    let who = pid.map_or_else(
        || "the previous overseer daemon".to_string(),
        |pid| format!("the previous overseer daemon (pid {pid})"),
    );
    Error::Command {
        context: "restart launchd overseer service",
        stderr: format!(
            "{who} is still running; refusing to start a second daemon. Once it exits on its own, run `robco restart` again."
        ),
    }
}

/// `bootstrap` succeeding only proves launchd loaded *a* job under the
/// label — if the old process never actually exited that is the same
/// evidence, since it is still what answers `probe_state() == Loaded`.
/// Confirm a genuinely new process took over: `PidGuard::acquire` writes the
/// daemon's own pid as the first thing `run_daemon` does, so this needs no
/// wait for a full pass — only for the new process to exist (dropr:483).
fn verify_new_process(old_pid: Option<u32>) -> Result<()> {
    verify_new_process_with(old_pid, SettleBudget::launchd(), daemon_pid, process_alive)
}

/// [`verify_new_process`] split from its real pidfile read and `kill -0`
/// check for the same reason as [`wait_for_pid_exit_with`].
fn verify_new_process_with(
    old_pid: Option<u32>,
    budget: SettleBudget,
    mut current_pid: impl FnMut() -> Option<u32>,
    mut alive: impl FnMut(u32) -> bool,
) -> Result<()> {
    for attempt in 0..budget.polls {
        if attempt > 0 {
            std::thread::sleep(budget.poll_interval);
        }
        if let Some(pid) = current_pid()
            && Some(pid) != old_pid
            && alive(pid)
        {
            return Ok(());
        }
    }
    Err(Error::Command {
        context: "confirm overseer restart",
        stderr: "launchd reports the service loaded, but no new daemon process took over the pidfile; the previous binary may still be the one running".into(),
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

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;
