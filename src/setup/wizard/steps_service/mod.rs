mod plan;
mod probe;

use std::io::{BufRead, Write};

use crate::{Result, config::Config};

#[cfg(target_os = "macos")]
use crate::Error;

#[cfg(target_os = "macos")]
use self::plan::{BootstrapPlan, SetenvPlan};
#[cfg(target_os = "macos")]
use self::probe::ServiceState;
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
        let bootstrap = BootstrapPlan {
            execute: self.bootstrap.execute && !defer_bootstrap,
            ..self.bootstrap
        };
        if defer_bootstrap {
            writeln!(
                output,
                "Automatic service loading deferred: set the Discord token first."
            )?;
        }
        if let Some(setenv) = self.setenv {
            setenv.apply(output)?;
        }
        bootstrap.apply(output)
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn configure<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    config: &Config,
) -> Result<Option<ServicePlan>> {
    use crate::overseer::command::write_service_plist;

    let service_probe = probe::run();
    if !confirm_service(input, output, service_probe.state)? {
        writeln!(output, "▌ robco ▸ launchd ··········· skipped")?;
        return Ok(None);
    }
    let path = write_service_plist()?;
    writeln!(output, "▌ robco ▸ launchd ··········· plist written")?;
    let uid = service_probe
        .uid
        .map_or_else(|| command_stdout("id", &["-u"]), Ok)?;
    let domain = format!("gui/{uid}");
    let execute = prompt::confirm(input, output, "Load the service now?", false)?;
    let setenv = discord_env_plan(input, output, config)?;
    Ok(Some(ServicePlan {
        setenv,
        bootstrap: BootstrapPlan {
            domain,
            path,
            execute,
        },
    }))
}

#[cfg(target_os = "macos")]
fn confirm_service<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    state: ServiceState,
) -> Result<bool> {
    let (label, default) = match state {
        ServiceState::NotInstalled => ("Install Overseer launchd service?", false),
        ServiceState::Unloaded => ("Load the installed Overseer service?", true),
        ServiceState::Loaded => (
            "Reload the running Overseer service? (picks up the upgraded binary)",
            true,
        ),
    };
    prompt::confirm(input, output, label, default)
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::io::Cursor;

    use super::{ServiceState, confirm_service};

    #[test]
    fn service_prompt_matches_current_state() {
        let cases = [
            (
                ServiceState::NotInstalled,
                "Install Overseer launchd service? [y/N]: ",
                false,
            ),
            (
                ServiceState::Unloaded,
                "Load the installed Overseer service? [Y/n]: ",
                true,
            ),
            (
                ServiceState::Loaded,
                "Reload the running Overseer service? \
                 (picks up the upgraded binary) [Y/n]: ",
                true,
            ),
        ];

        for (state, expected_prompt, expected_default) in cases {
            let mut input = Cursor::new(b"\n");
            let mut output = Vec::new();

            let accepted = confirm_service(&mut input, &mut output, state).unwrap();
            let output = String::from_utf8(output).unwrap();

            assert_eq!(output, format!("▌ robco ▸ {expected_prompt}"));
            assert_eq!(accepted, expected_default);
        }
    }
}
