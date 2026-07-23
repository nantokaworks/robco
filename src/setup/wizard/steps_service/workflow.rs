#[cfg(target_os = "macos")]
use std::io::{BufRead, Write};

#[cfg(target_os = "macos")]
use crate::{
    Result,
    overseer::command::{ActiveWorkers, load_active_workers},
};

#[cfg(target_os = "macos")]
use super::{
    plan::BootstrapMode,
    probe::{self, ServiceState},
    prompt,
};

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Caller {
    Wizard,
    InstallCommand,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
pub(super) struct WorkflowPlan {
    pub(super) write_plist: bool,
    pub(super) execute: bool,
    pub(super) mode: BootstrapMode,
}

#[cfg(target_os = "macos")]
pub(super) fn prepare<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    caller: Caller,
) -> Result<WorkflowPlan> {
    prepare_with(
        input,
        output,
        caller,
        || probe::run().state,
        load_active_workers,
    )
}

#[cfg(target_os = "macos")]
pub(super) fn prepare_with<R, W, P, L>(
    input: &mut R,
    output: &mut W,
    caller: Caller,
    probe: P,
    load_workers: L,
) -> Result<WorkflowPlan>
where
    R: BufRead,
    W: Write,
    P: FnOnce() -> ServiceState,
    L: FnOnce() -> Result<ActiveWorkers>,
{
    let state = probe();
    render_state(output, state)?;
    match (caller, state) {
        (Caller::InstallCommand, ServiceState::NotInstalled | ServiceState::Unloaded) => {
            Ok(WorkflowPlan {
                write_plist: true,
                execute: false,
                mode: BootstrapMode::Load,
            })
        }
        (Caller::InstallCommand, ServiceState::Loaded) => {
            if !prompt::confirm(input, output, "Reload the running Overseer service?", false)? {
                return left_as_is(output);
            }
            guard_reload(input, output, load_workers)
        }
        (Caller::Wizard, state) => {
            let (label, default) = service_prompt(state);
            if !prompt::confirm(input, output, label, default)? {
                writeln!(output, "▌ robco ▸ launchd ··········· skipped")?;
                return Ok(skip());
            }
            match state {
                ServiceState::NotInstalled => {
                    let execute = prompt::confirm(input, output, "Load the service now?", false)?;
                    Ok(WorkflowPlan {
                        write_plist: true,
                        execute,
                        mode: BootstrapMode::Load,
                    })
                }
                ServiceState::Unloaded => Ok(WorkflowPlan {
                    write_plist: true,
                    execute: true,
                    mode: BootstrapMode::Load,
                }),
                ServiceState::Loaded => guard_reload(input, output, load_workers),
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn guard_reload<R, W, L>(input: &mut R, output: &mut W, load_workers: L) -> Result<WorkflowPlan>
where
    R: BufRead,
    W: Write,
    L: FnOnce() -> Result<ActiveWorkers>,
{
    let workers = match load_workers() {
        Ok(workers) if workers.count == 0 => {
            return Ok(WorkflowPlan {
                write_plist: true,
                execute: true,
                mode: BootstrapMode::Reload,
            });
        }
        Ok(workers) => WorkerState::Active(workers),
        Err(_) => WorkerState::Unknown,
    };
    if let Some(message) = warning_message(&workers) {
        write!(output, "{message}")?;
    }
    if prompt::confirm(input, output, "Reload anyway?", false)? {
        Ok(WorkflowPlan {
            write_plist: true,
            execute: true,
            mode: BootstrapMode::Reload,
        })
    } else {
        left_as_is(output)
    }
}

#[cfg(target_os = "macos")]
fn left_as_is<W: Write>(output: &mut W) -> Result<WorkflowPlan> {
    writeln!(
        output,
        "▌ robco ▸ launchd ··········· service left as-is; rerun to reload later"
    )?;
    Ok(skip())
}

#[cfg(target_os = "macos")]
fn skip() -> WorkflowPlan {
    WorkflowPlan {
        write_plist: false,
        execute: false,
        mode: BootstrapMode::Load,
    }
}

#[cfg(target_os = "macos")]
fn service_prompt(state: ServiceState) -> (&'static str, bool) {
    match state {
        ServiceState::NotInstalled => ("Install Overseer launchd service?", false),
        ServiceState::Unloaded => ("Load the installed Overseer service?", true),
        ServiceState::Loaded => (
            "Reload the running Overseer service? (picks up the upgraded binary)",
            true,
        ),
    }
}

#[cfg(target_os = "macos")]
fn render_state<W: Write>(output: &mut W, state: ServiceState) -> Result<()> {
    let label = match state {
        ServiceState::NotInstalled => "not installed",
        ServiceState::Unloaded => "installed, not loaded",
        ServiceState::Loaded => "loaded",
    };
    writeln!(output, "▌ robco ▸ launchd ··········· {label}")?;
    Ok(())
}

#[cfg(target_os = "macos")]
pub(super) enum WorkerState {
    Active(ActiveWorkers),
    Unknown,
}

#[cfg(target_os = "macos")]
pub(super) fn warning_message(state: &WorkerState) -> Option<String> {
    match state {
        WorkerState::Active(workers) if workers.count == 0 => None,
        WorkerState::Active(workers) => Some(format!(
            "▌ robco ▸ WARNING ·········· Overseer is working: {} active workers\n\
             \x20                            {:?}\n\
             \x20                            Reloading interrupts the current pass.\n",
            workers.count, workers.repos
        )),
        WorkerState::Unknown => Some(
            "▌ robco ▸ WARNING ·········· Overseer worker state could not be determined\n\
             \x20                            The ledger is missing or unreadable.\n\
             \x20                            Reloading may interrupt the current pass.\n"
                .into(),
        ),
    }
}
