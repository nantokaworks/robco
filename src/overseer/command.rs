use std::{collections::HashSet, fs, process::Command, thread, time::Duration};

use super::{
    exec::{process_alive, run_timeout},
    is_overseer_child,
    logging::{self, DecisionEntry, DecisionKind},
    pidfile_path,
    runtime_request::{self, RuntimeRequest},
};
use crate::{
    Result,
    cli::{OverseerArgs, OverseerCommand},
    config::Config,
    registry::Registry,
};

mod compact;
mod escalation;
mod inbox;
mod service;
mod settings;
mod status;

use compact::compact_decisions;
pub(crate) use escalation::escalate_workers;
use inbox::clear_inbox;
#[cfg(target_os = "macos")]
pub(crate) use service::write_service_plist;
use service::{ServiceState, StopOutcome, install_service, service_state};
pub(crate) use settings::set_runtime;
use settings::{autonomy_level, notify_channel, protection_mode, set};
use status::status;

#[cfg(target_os = "macos")]
pub(crate) use super::ledger::ActiveWorkers;
#[cfg(target_os = "macos")]
use super::ledger::Ledger;
pub(crate) use super::ledger::terminal;

pub fn run(args: OverseerArgs, config: &Config) -> Result<()> {
    match args.command {
        OverseerCommand::Run => unreachable!("async overseer run handled by main"),
        OverseerCommand::Status(args) => status(config, args.debug),
        OverseerCommand::Stop => stop(),
        OverseerCommand::Start => start(),
        OverseerCommand::Restart => restart(),
        OverseerCommand::Set(args) => set(args.setting, args.value.enabled()),
        OverseerCommand::NotifyChannel(args) => {
            notify_channel(if args.clear { None } else { args.channel_id })
        }
        OverseerCommand::Protection(args) => protection_mode(args.mode),
        OverseerCommand::Autonomy(args) => autonomy_level(args.level),
        OverseerCommand::Panic => panic_stop(),
        OverseerCommand::ClearInbox => clear_inbox(),
        OverseerCommand::InstallService => install_service(),
        OverseerCommand::CompactDecisions(args) => compact_decisions(args.dry_run),
    }
}

/// Outcome of a stop attempt, shared between the CLI (`stop()`, which prints
/// it) and the TUI (`App::stop_daemon`, which localizes it) — see dropr:412.
pub(crate) enum StopAttempt {
    NotRunning,
    Stopped,
    StillShuttingDown,
}

/// Outcome of a start attempt. See [`StopAttempt`].
pub(crate) enum StartAttempt {
    /// No launchd service is installed and this OS has no other launchd-free
    /// equivalent; the caller has to run the daemon by hand.
    NotInstalled,
    /// launchd service management does not exist on this OS at all.
    Unsupported,
    AlreadyRunning,
    Started,
}

/// Outcome of a restart attempt. See [`StopAttempt`].
pub(crate) enum RestartAttempt {
    NotInstalled,
    Unsupported,
    /// The service was not running; restart just started it.
    Started,
    Restarted,
}

/// Durably stop the daemon: bootout the launchd service if one is loaded —
/// a `KeepAlive` job only leaves the domain that way, since a bare `SIGTERM`
/// gets it respawned — else fall back to the manual pidfile/SIGTERM path for
/// a daemon started with `robco overseer run` directly.
pub(crate) fn stop_daemon() -> Result<StopAttempt> {
    stop_daemon_with(service_state, service::stop_service, stop_manual)
}

fn stop_daemon_with<S, T, M>(state: S, stop_service: T, stop_manual: M) -> Result<StopAttempt>
where
    S: FnOnce() -> ServiceState,
    T: FnOnce() -> Result<StopOutcome>,
    M: FnOnce() -> Result<StopAttempt>,
{
    if state() == ServiceState::Loaded {
        return Ok(match stop_service()? {
            StopOutcome::Stopped => StopAttempt::Stopped,
            StopOutcome::StillShuttingDown => StopAttempt::StillShuttingDown,
        });
    }
    stop_manual()
}

fn stop_manual() -> Result<StopAttempt> {
    let Some(pid) = read_pid() else {
        return Ok(StopAttempt::NotRunning);
    };
    let mut command = Command::new("kill");
    command.args(["-TERM", &pid.to_string()]);
    let output = run_timeout(command, Duration::from_secs(2))?;
    if !output.status.success() {
        return Err(std::io::Error::other("failed to signal overseer daemon").into());
    }
    for _ in 0..20 {
        if !process_alive(pid) {
            return Ok(StopAttempt::Stopped);
        }
        thread::sleep(Duration::from_millis(100));
    }
    Ok(StopAttempt::StillShuttingDown)
}

/// Start the daemon: bootstrap the launchd service if one is installed but
/// not loaded. There is no manual-start equivalent — a daemon not running
/// under launchd has to be launched with `robco overseer run` by hand, which
/// this only names in the message.
pub(crate) fn start_daemon() -> Result<StartAttempt> {
    start_daemon_with(service_state, service::start_service)
}

fn start_daemon_with<S, T>(state: S, start_service: T) -> Result<StartAttempt>
where
    S: FnOnce() -> ServiceState,
    T: FnOnce() -> Result<()>,
{
    match state() {
        ServiceState::NotInstalled => Ok(StartAttempt::NotInstalled),
        ServiceState::Unsupported => Ok(StartAttempt::Unsupported),
        ServiceState::Loaded => Ok(StartAttempt::AlreadyRunning),
        ServiceState::Unloaded => {
            start_service()?;
            Ok(StartAttempt::Started)
        }
    }
}

/// Restart the daemon: bootout-then-bootstrap when the service is loaded,
/// or a plain bootstrap when it is installed but not currently running.
pub(crate) fn restart_daemon() -> Result<RestartAttempt> {
    restart_daemon_with(
        service_state,
        service::start_service,
        service::restart_service,
    )
}

fn restart_daemon_with<S, T, R>(
    state: S,
    start_service: T,
    restart_service: R,
) -> Result<RestartAttempt>
where
    S: FnOnce() -> ServiceState,
    T: FnOnce() -> Result<()>,
    R: FnOnce() -> Result<()>,
{
    match state() {
        ServiceState::NotInstalled => Ok(RestartAttempt::NotInstalled),
        ServiceState::Unsupported => Ok(RestartAttempt::Unsupported),
        ServiceState::Unloaded => {
            start_service()?;
            Ok(RestartAttempt::Started)
        }
        ServiceState::Loaded => {
            restart_service()?;
            Ok(RestartAttempt::Restarted)
        }
    }
}

const NO_SERVICE_HINT: &str = "no launchd service installed; run `robco overseer install-service` to install and load it, or `robco overseer run` to run the daemon in the foreground";
const UNSUPPORTED_HINT: &str = "launchd service management is unavailable on this OS; run `robco overseer run` to run the daemon in the foreground";

fn stop() -> Result<()> {
    match stop_daemon()? {
        StopAttempt::NotRunning => println!("overseer is not running"),
        StopAttempt::Stopped => println!("overseer stopped"),
        StopAttempt::StillShuttingDown => println!(
            "overseer shutdown requested; a pass may still be finishing, but it will not restart automatically"
        ),
    }
    Ok(())
}

fn start() -> Result<()> {
    match start_daemon()? {
        StartAttempt::NotInstalled => println!("{NO_SERVICE_HINT}"),
        StartAttempt::Unsupported => println!("{UNSUPPORTED_HINT}"),
        StartAttempt::AlreadyRunning => println!("overseer is already running"),
        StartAttempt::Started => println!("overseer started"),
    }
    Ok(())
}

fn restart() -> Result<()> {
    match restart_daemon()? {
        RestartAttempt::NotInstalled => println!("{NO_SERVICE_HINT}"),
        RestartAttempt::Unsupported => println!("{UNSUPPORTED_HINT}"),
        RestartAttempt::Started => println!("overseer started"),
        RestartAttempt::Restarted => println!("overseer restarted"),
    }
    Ok(())
}

fn panic_stop() -> Result<()> {
    panic_stop_attributed("cli", None)?;
    println!("overseer panic stop complete");
    Ok(())
}

pub(crate) fn panic_stop_attributed(source: &str, user_id: Option<&str>) -> Result<()> {
    let registry = Registry::load()?;
    let mut killed_ids = HashSet::new();
    for agent in registry
        .repos
        .iter()
        .flat_map(|repo| &repo.agents)
        .filter(|agent| is_overseer_child(agent.parent_agent_id.as_deref()))
    {
        let mut command = Command::new("tmux");
        command.args(["kill-session", "-t", &format!("={}", agent.tmux_session)]);
        if run_timeout(command, Duration::from_secs(5)).is_ok() {
            killed_ids.insert(agent.id.clone());
        }
    }
    runtime_request::enqueue(RuntimeRequest::PanicEscalate {
        source: source.into(),
        agent_ids: killed_ids.into_iter().collect(),
        at: chrono::Utc::now(),
    })?;
    let mut entry = DecisionEntry::new(DecisionKind::Escalate, "panic stop: workers terminated");
    entry.source = Some(source.into());
    entry.user_id = user_id.map(str::to_owned);
    logging::append(&entry)?;
    Ok(())
}

fn read_pid() -> Option<u32> {
    fs::read_to_string(pidfile_path().ok()?)
        .ok()?
        .trim()
        .parse()
        .ok()
}
fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}
/// Read by the macOS service wizard only; the launchd bootstrap step is the
/// single caller.
#[cfg(target_os = "macos")]
pub(crate) fn load_active_workers() -> Result<ActiveWorkers> {
    let raw = fs::read_to_string(crate::overseer::ledger_path()?)?;
    let ledger: Ledger = serde_json::from_str(&raw)?;
    Ok(ledger.active_workers())
}

#[cfg(test)]
mod daemon_lifecycle_tests {
    use super::*;

    fn unreachable_start_service() -> Result<()> {
        unreachable!("start_service must not run when the service is not Unloaded")
    }

    fn unreachable_restart_service() -> Result<()> {
        unreachable!("restart_service must not run when the service is not Loaded")
    }

    #[test]
    fn a_loaded_service_stops_through_bootout_instead_of_the_manual_path() {
        let outcome = stop_daemon_with(
            || ServiceState::Loaded,
            || Ok(StopOutcome::Stopped),
            || unreachable!("the manual pidfile path must not run when a service is loaded"),
        )
        .unwrap();

        assert!(matches!(outcome, StopAttempt::Stopped));
    }

    #[test]
    fn a_bootout_that_has_not_landed_yet_reports_still_shutting_down_not_failure() {
        // The daemon may still be finishing a merge pass past the settle
        // window; bootout is already durable at that point, so this must
        // read as an honest in-progress status, not a flaky failure.
        let outcome = stop_daemon_with(
            || ServiceState::Loaded,
            || Ok(StopOutcome::StillShuttingDown),
            || unreachable!("the manual pidfile path must not run when a service is loaded"),
        )
        .unwrap();

        assert!(matches!(outcome, StopAttempt::StillShuttingDown));
    }

    #[test]
    fn an_absent_service_falls_back_to_the_manual_stop_path() {
        for state in [
            ServiceState::NotInstalled,
            ServiceState::Unloaded,
            ServiceState::Unsupported,
        ] {
            let outcome = stop_daemon_with(
                move || state,
                || unreachable!("stop_service must not run when the service is not Loaded"),
                || Ok(StopAttempt::NotRunning),
            )
            .unwrap();

            assert!(matches!(outcome, StopAttempt::NotRunning), "{state:?}");
        }
    }

    #[test]
    fn starting_with_no_service_installed_names_the_manual_alternative() {
        let outcome =
            start_daemon_with(|| ServiceState::NotInstalled, unreachable_start_service).unwrap();

        assert!(matches!(outcome, StartAttempt::NotInstalled));
    }

    #[test]
    fn starting_on_an_unsupported_os_names_the_manual_alternative() {
        let outcome =
            start_daemon_with(|| ServiceState::Unsupported, unreachable_start_service).unwrap();

        assert!(matches!(outcome, StartAttempt::Unsupported));
    }

    #[test]
    fn starting_an_already_loaded_service_is_a_no_op() {
        let outcome =
            start_daemon_with(|| ServiceState::Loaded, unreachable_start_service).unwrap();

        assert!(matches!(outcome, StartAttempt::AlreadyRunning));
    }

    #[test]
    fn starting_an_unloaded_service_bootstraps_it() {
        let outcome = start_daemon_with(|| ServiceState::Unloaded, || Ok(())).unwrap();

        assert!(matches!(outcome, StartAttempt::Started));
    }

    #[test]
    fn restarting_a_loaded_service_boots_it_out_and_back_in() {
        let outcome = restart_daemon_with(
            || ServiceState::Loaded,
            unreachable_start_service,
            || Ok(()),
        )
        .unwrap();

        assert!(matches!(outcome, RestartAttempt::Restarted));
    }

    #[test]
    fn restarting_an_unloaded_service_only_bootstraps_it() {
        let outcome = restart_daemon_with(
            || ServiceState::Unloaded,
            || Ok(()),
            unreachable_restart_service,
        )
        .unwrap();

        assert!(matches!(outcome, RestartAttempt::Started));
    }

    #[test]
    fn restarting_with_no_service_installed_names_the_manual_alternative() {
        let outcome = restart_daemon_with(
            || ServiceState::NotInstalled,
            unreachable_start_service,
            unreachable_restart_service,
        )
        .unwrap();

        assert!(matches!(outcome, RestartAttempt::NotInstalled));
    }
}
