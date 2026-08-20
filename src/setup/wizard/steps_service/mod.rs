mod plan;
// `probe` and `settle` hold the launchd primitives `overseer::command::service::control`
// reuses for `robco stop|start|restart` (dropr:412), so both need to be
// reachable outside this module tree; `plan` and `workflow` stay private to the
// interactive install/reload flow.
pub(crate) mod probe;
pub(crate) mod settle;
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

/// Closing check for the wizard: an unloaded service means nothing merges,
/// launches a named task, or answers Discord/MCP commands. The wizard
/// otherwise ends on an unconditional `setup complete`, which reads as "the
/// daemon is up" even when the operator declined the load prompt.
#[cfg(target_os = "macos")]
pub(crate) fn warn_if_service_down<W: Write>(output: &mut W, _config: &Config) -> Result<()> {
    warn_if_service_down_with(output, || probe::run().state)
}

#[cfg(target_os = "macos")]
fn warn_if_service_down_with<W, P>(output: &mut W, probe: P) -> Result<()>
where
    W: Write,
    P: FnOnce() -> probe::ServiceState,
{
    if probe() == probe::ServiceState::Loaded {
        return Ok(());
    }
    writeln!(
        output,
        "▌ robco ▸ WARNING ·········· the Overseer daemon is not running — nothing merges, \
         launches a named task, or answers Discord/MCP commands until it is. Start it with \
         `robco daemon`, or install the always-on service with \
         `robco service install`."
    )?;
    Ok(())
}

/// Mirrors the resolution order `resolve_token` uses for the running bot
/// (`src/overseer/discord/mod.rs`): the process environment wins, then the
/// session env file, a blank value counting as absent at both layers. When the
/// token resolves from the file the daemon already has everything it needs, so
/// there is nothing to copy into launchd and no reason to defer the load.
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
    if let Some(value) = std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return if prompt::confirm(
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
        };
    }
    let resolved_from_file = crate::overseer::session::env::env_file_path(config)
        .and_then(|path| crate::overseer::session::env::lookup_var(&path, name))
        .is_some_and(|value| !value.trim().is_empty());
    if resolved_from_file {
        return Ok(None);
    }
    Ok(Some(SetenvPlan {
        name: name.clone(),
        value: None,
    }))
}

#[cfg(all(test, target_os = "macos"))]
mod discord_env_plan_tests {
    use super::*;

    fn discord_config(token_env: &str, env_file: std::path::PathBuf) -> Config {
        let mut config = Config::default();
        config.overseer.discord.enabled = true;
        config.overseer.discord.token_env = token_env.to_string();
        config.overseer.session_env_file = Some(env_file);
        config
    }

    fn plan(config: &Config) -> Option<SetenvPlan> {
        let mut input: &[u8] = b"";
        let mut output = Vec::new();
        discord_env_plan(&mut input, &mut output, config).unwrap()
    }

    #[test]
    fn no_token_anywhere_still_defers_with_the_setenv_hint() {
        let temp = tempfile::tempdir().unwrap();
        let config = discord_config("ROBCO_TEST_DISCORD_TOKEN_ABSENT", temp.path().join("env"));

        assert!(plan(&config).is_some_and(|plan| plan.value.is_none()));
    }

    #[test]
    fn a_token_in_the_session_env_file_resolves_with_no_plan_at_all() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("env");
        std::fs::write(&file, "ROBCO_TEST_DISCORD_TOKEN_FILE=from-file\n").unwrap();
        let config = discord_config("ROBCO_TEST_DISCORD_TOKEN_FILE", file);

        assert!(plan(&config).is_none());
    }

    #[test]
    fn a_blank_value_in_the_session_env_file_is_treated_as_absent() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("env");
        std::fs::write(&file, "ROBCO_TEST_DISCORD_TOKEN_BLANK=   \n").unwrap();
        let config = discord_config("ROBCO_TEST_DISCORD_TOKEN_BLANK", file);

        assert!(plan(&config).is_some_and(|plan| plan.value.is_none()));
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
