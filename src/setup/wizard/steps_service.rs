use std::io::{BufRead, Write};

use crate::{Result, config::Config};

#[cfg(target_os = "macos")]
use crate::Error;

#[cfg(target_os = "macos")]
use super::prompt;

#[cfg(target_os = "macos")]
pub(crate) struct ServicePlan {
    setenv: Option<SetenvPlan>,
    bootstrap: BootstrapPlan,
}

#[cfg(target_os = "macos")]
struct SetenvPlan {
    name: String,
    value: Option<String>,
}

#[cfg(target_os = "macos")]
struct BootstrapPlan {
    domain: String,
    path: std::path::PathBuf,
    execute: bool,
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
impl SetenvPlan {
    fn apply<W: Write>(self, output: &mut W) -> Result<()> {
        use std::{process::Command, time::Duration};

        use crate::overseer::exec::run_timeout;

        let Some(value) = self.value else {
            writeln!(
                output,
                "  launchctl setenv {} \"${}\"",
                self.name, self.name
            )?;
            return Ok(());
        };
        let mut command = Command::new("launchctl");
        command.args(["setenv", &self.name, &value]);
        let result = run_timeout(command, Duration::from_secs(5))?;
        if !result.status.success() {
            return Err(Error::Wizard("launchctl setenv failed".into()));
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
impl BootstrapPlan {
    fn apply<W: Write>(self, output: &mut W) -> Result<()> {
        use std::{process::Command, time::Duration};

        use crate::overseer::exec::run_timeout;

        if !self.execute {
            writeln!(
                output,
                "  launchctl bootstrap {} {}",
                self.domain,
                self.path.display()
            )?;
            return Ok(());
        }
        let mut command = Command::new("launchctl");
        command.args(["bootstrap", &self.domain]).arg(&self.path);
        let result = run_timeout(command, Duration::from_secs(5))?;
        if !result.status.success() {
            return Err(Error::Wizard(format!(
                "launchctl bootstrap failed: {}",
                String::from_utf8_lossy(&result.stderr).trim()
            )));
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn configure<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    config: &Config,
) -> Result<Option<ServicePlan>> {
    use crate::overseer::command::write_service_plist;

    if !prompt::confirm(input, output, "Install Overseer launchd service?", false)? {
        writeln!(output, "▌ robco ▸ launchd ··········· skipped")?;
        return Ok(None);
    }
    let path = write_service_plist()?;
    writeln!(output, "▌ robco ▸ launchd ··········· plist written")?;
    let uid = command_stdout("id", &["-u"])?;
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
    use std::path::PathBuf;

    use super::{BootstrapPlan, ServicePlan, SetenvPlan};

    #[test]
    fn copyable_setenv_command_precedes_bootstrap() {
        let plan = ServicePlan {
            setenv: Some(SetenvPlan {
                name: "DISCORD_TOKEN".into(),
                value: None,
            }),
            bootstrap: BootstrapPlan {
                domain: "gui/501".into(),
                path: PathBuf::from("/tmp/robco.plist"),
                execute: false,
            },
        };
        let mut output = Vec::new();
        plan.apply(&mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(
            output.find("launchctl setenv").unwrap() < output.find("launchctl bootstrap").unwrap()
        );
    }

    #[test]
    fn manual_setenv_defers_accepted_bootstrap() {
        let plan = ServicePlan {
            setenv: Some(SetenvPlan {
                name: "DISCORD_TOKEN".into(),
                value: None,
            }),
            bootstrap: BootstrapPlan {
                domain: "gui/501".into(),
                path: PathBuf::from("/tmp/robco.plist"),
                execute: true,
            },
        };
        let mut output = Vec::new();
        plan.apply(&mut output).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains("Automatic service loading deferred"));
        assert!(
            output.find("launchctl setenv").unwrap() < output.find("launchctl bootstrap").unwrap()
        );
    }
}
