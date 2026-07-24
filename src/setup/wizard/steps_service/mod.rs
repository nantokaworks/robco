mod plan;
mod probe;
mod workflow;
#[cfg(all(test, target_os = "macos"))]
mod workflow_tests;

use std::io::{BufRead, Write};

#[cfg(target_os = "macos")]
use std::io;

use crate::{Result, config::Config};

#[cfg(target_os = "macos")]
use crate::Error;

#[cfg(target_os = "macos")]
use self::plan::{BootstrapPlan, SetenvPlan};
#[cfg(target_os = "macos")]
use self::workflow::{Caller, WorkflowPlan};
#[cfg(target_os = "macos")]
use super::prompt;

#[cfg(target_os = "macos")]
pub(crate) struct ServicePlan {
    setenv: Option<SetenvPlan>,
    bootstrap: BootstrapPlan,
}

#[cfg(target_os = "macos")]
impl ServicePlan {
    pub(crate) fn apply<W: Write>(self, output: &mut W) -> Result<()> {
        let defer_bootstrap = self.bootstrap.execute
            && self
                .setenv
                .as_ref()
                .is_some_and(|setenv| setenv.value.is_none());
        if defer_bootstrap {
            writeln!(
                output,
                "Automatic service loading or reloading deferred: set the Discord token first."
            )?;
        }
        if let Some(setenv) = self.setenv {
            setenv.apply(output)?;
        }
        if defer_bootstrap {
            return Ok(());
        }
        self.bootstrap.apply(output)
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn configure<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    config: &Config,
) -> Result<Option<ServicePlan>> {
    let workflow = workflow::prepare(input, output, Caller::Wizard)?;
    let Some(bootstrap) = materialize(workflow, output)? else {
        return Ok(None);
    };
    let setenv = discord_env_plan(input, output, config)?;
    Ok(Some(ServicePlan { setenv, bootstrap }))
}

#[cfg(target_os = "macos")]
fn materialize<W: Write>(workflow: WorkflowPlan, output: &mut W) -> Result<Option<BootstrapPlan>> {
    materialize_with(
        workflow,
        output,
        crate::overseer::command::write_service_plist,
    )
}

#[cfg(target_os = "macos")]
fn materialize_with<W, F>(
    workflow: WorkflowPlan,
    output: &mut W,
    write_plist: F,
) -> Result<Option<BootstrapPlan>>
where
    W: Write,
    F: FnOnce() -> Result<std::path::PathBuf>,
{
    if !workflow.write_plist {
        return Ok(None);
    }
    let path = write_plist()?;
    writeln!(output, "▌ robco ▸ launchd ··········· plist written")?;
    let uid = command_stdout("id", &["-u"])?;
    Ok(Some(BootstrapPlan {
        domain: format!("gui/{uid}"),
        path,
        executable: std::env::current_exe()?,
        execute: workflow.execute,
        mode: workflow.mode,
    }))
}

/// Closing check for the wizard: dispatch being on while the service is not
/// loaded means the toggle is set but nothing consumes ready tasks. The wizard
/// otherwise ends on an unconditional `setup complete`, which reads as "the
/// daemon is up" even when the operator declined the load prompt.
#[cfg(target_os = "macos")]
pub(crate) fn warn_if_service_down<W: Write>(output: &mut W, config: &Config) -> Result<()> {
    warn_if_service_down_with(output, config.overseer.dispatch_enabled, || {
        probe::run().state
    })
}

#[cfg(target_os = "macos")]
fn warn_if_service_down_with<W, P>(output: &mut W, dispatch_enabled: bool, probe: P) -> Result<()>
where
    W: Write,
    P: FnOnce() -> probe::ServiceState,
{
    if !dispatch_enabled || probe() == probe::ServiceState::Loaded {
        return Ok(());
    }
    writeln!(
        output,
        "▌ robco ▸ WARNING ·········· {}",
        crate::overseer::DISPATCH_WITHOUT_DAEMON_HINT
    )?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn discord_env_plan<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    config: &Config,
) -> Result<Option<SetenvPlan>> {
    if !config.overseer.discord.enabled {
        return Ok(None);
    }
    let name = &config.overseer.discord.token_env;
    let Ok(value) = std::env::var(name) else {
        return Ok(Some(SetenvPlan {
            name: name.clone(),
            value: None,
        }));
    };
    if prompt::confirm(
        input,
        output,
        &format!("Copy {name} from this process into launchd?"),
        false,
    )? {
        Ok(Some(SetenvPlan {
            name: name.clone(),
            value: Some(value),
        }))
    } else {
        Ok(Some(SetenvPlan {
            name: name.clone(),
            value: None,
        }))
    }
}

#[cfg(target_os = "macos")]
fn command_stdout(program: &str, args: &[&str]) -> Result<String> {
    use std::{process::Command, time::Duration};

    use crate::overseer::exec::run_timeout;

    let mut command = Command::new(program);
    command.args(args);
    let output = run_timeout(command, Duration::from_secs(5))?;
    if !output.status.success() {
        return Err(Error::Wizard(format!("{program} failed")));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "macos")]
pub(crate) fn install_service() -> Result<()> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    let workflow = workflow::prepare(&mut input, &mut output, Caller::InstallCommand)?;
    if let Some(plan) = materialize(workflow, &mut output)? {
        plan.apply(&mut output)?;
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(crate) struct ServicePlan;

#[cfg(not(target_os = "macos"))]
impl ServicePlan {
    pub(crate) fn apply<W: Write>(self, _output: &mut W) -> Result<()> {
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn configure<R: BufRead, W: Write>(
    _input: &mut R,
    output: &mut W,
    _config: &Config,
) -> Result<Option<ServicePlan>> {
    writeln!(
        output,
        "▌ robco ▸ launchd ··········· unavailable on this OS"
    )?;
    Ok(None)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn warn_if_service_down<W: Write>(_output: &mut W, _config: &Config) -> Result<()> {
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn install_service() -> Result<()> {
    Err(crate::Error::Wizard(
        "launchd service installation is unavailable on this OS".into(),
    ))
}
