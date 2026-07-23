#[cfg(target_os = "macos")]
use std::io::Write;

#[cfg(target_os = "macos")]
use crate::{Error, Result};

#[cfg(target_os = "macos")]
pub(super) struct SetenvPlan {
    pub(super) name: String,
    pub(super) value: Option<String>,
}

#[cfg(target_os = "macos")]
pub(super) struct BootstrapPlan {
    pub(super) domain: String,
    pub(super) path: std::path::PathBuf,
    pub(super) execute: bool,
}

#[cfg(target_os = "macos")]
impl SetenvPlan {
    pub(super) fn apply<W: Write>(self, output: &mut W) -> Result<()> {
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
    pub(super) fn apply<W: Write>(self, output: &mut W) -> Result<()> {
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::path::PathBuf;

    use super::{BootstrapPlan, SetenvPlan};
    use crate::setup::wizard::steps_service::ServicePlan;

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
