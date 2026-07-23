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
    pub(super) executable: std::path::PathBuf,
    pub(super) execute: bool,
    pub(super) mode: BootstrapMode,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
pub(super) enum BootstrapMode {
    Load,
    Reload,
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
        self.apply_with(
            output,
            |args| {
                use std::{process::Command, time::Duration};

                let mut command = Command::new("launchctl");
                command.args(args);
                crate::overseer::exec::run_timeout(command, Duration::from_secs(5))
            },
            || super::probe::run().state,
        )
    }

    fn apply_with<W, F, P>(self, output: &mut W, mut run: F, probe: P) -> Result<()>
    where
        W: Write,
        F: FnMut(&[std::ffi::OsString]) -> std::io::Result<std::process::Output>,
        P: FnOnce() -> super::probe::ServiceState,
    {
        if !self.execute {
            if matches!(self.mode, BootstrapMode::Reload) {
                writeln!(
                    output,
                    "  launchctl bootout {}/com.robco.overseer",
                    self.domain
                )?;
            }
            writeln!(
                output,
                "  launchctl bootstrap {} {}",
                self.domain,
                self.path.display()
            )?;
            return Ok(());
        }
        if matches!(self.mode, BootstrapMode::Reload) {
            let service = format!("{}/com.robco.overseer", self.domain);
            let result = run(&["bootout".into(), service.into()])?;
            if !result.status.success() && !service_is_absent(result.status.code(), &result.stderr)
            {
                return Err(Error::Wizard(format!(
                    "launchctl bootout failed: {}",
                    String::from_utf8_lossy(&result.stderr).trim()
                )));
            }
        }
        let result = run(&[
            "bootstrap".into(),
            self.domain.clone().into(),
            self.path.clone().into_os_string(),
        ])?;
        if !result.status.success() {
            return Err(Error::Wizard(format!(
                "launchctl bootstrap failed: {}",
                String::from_utf8_lossy(&result.stderr).trim()
            )));
        }
        if matches!(self.mode, BootstrapMode::Reload) {
            if probe() != super::probe::ServiceState::Loaded {
                return Err(Error::Wizard(
                    "launchd service did not come back after reload".into(),
                ));
            }
            writeln!(
                output,
                "▌ robco ▸ launchd ··········· reloaded ({})",
                self.executable.display()
            )?;
        }
        Ok(())
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::{
        ffi::OsString,
        os::unix::process::ExitStatusExt,
        path::PathBuf,
        process::{ExitStatus, Output},
    };

    use super::{BootstrapMode, BootstrapPlan, SetenvPlan, service_is_absent};
    use crate::setup::wizard::steps_service::{ServicePlan, probe::ServiceState};

    fn bootstrap_plan(execute: bool, mode: BootstrapMode) -> BootstrapPlan {
        BootstrapPlan {
            domain: "gui/501".into(),
            path: PathBuf::from("/tmp/robco.plist"),
            executable: PathBuf::from("/opt/homebrew/bin/robco"),
            execute,
            mode,
        }
    }

    #[test]
    fn copyable_setenv_command_precedes_bootstrap() {
        let plan = ServicePlan {
            setenv: Some(SetenvPlan {
                name: "DISCORD_TOKEN".into(),
                value: None,
            }),
            bootstrap: bootstrap_plan(false, BootstrapMode::Load),
        };
        let mut output = Vec::new();
        plan.apply(&mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(
            output.find("launchctl setenv").unwrap() < output.find("launchctl bootstrap").unwrap()
        );
    }

    #[test]
    fn manual_setenv_defers_accepted_reload() {
        let plan = ServicePlan {
            setenv: Some(SetenvPlan {
                name: "DISCORD_TOKEN".into(),
                value: None,
            }),
            bootstrap: bootstrap_plan(true, BootstrapMode::Reload),
        };
        let mut output = Vec::new();
        plan.apply(&mut output).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains("Automatic service loading or reloading deferred"));
        assert!(output.contains("launchctl setenv"));
        assert!(!output.contains("launchctl bootout"));
        assert!(!output.contains("launchctl bootstrap"));
    }

    #[test]
    fn reload_renders_bootout_before_bootstrap() {
        let mut output = Vec::new();
        bootstrap_plan(false, BootstrapMode::Reload)
            .apply(&mut output)
            .unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(
            output.find("launchctl bootout").unwrap() < output.find("launchctl bootstrap").unwrap()
        );
    }

    #[test]
    fn reload_executes_bootout_before_bootstrap() {
        let mut invocations = Vec::new();
        bootstrap_plan(true, BootstrapMode::Reload)
            .apply_with(
                &mut Vec::new(),
                |args| {
                    invocations.push(args.to_vec());
                    Ok(Output {
                        status: ExitStatus::from_raw(0),
                        stdout: Vec::new(),
                        stderr: Vec::new(),
                    })
                },
                || ServiceState::Loaded,
            )
            .unwrap();

        assert_eq!(
            invocations,
            vec![
                vec![
                    OsString::from("bootout"),
                    OsString::from("gui/501/com.robco.overseer")
                ],
                vec![
                    OsString::from("bootstrap"),
                    OsString::from("gui/501"),
                    OsString::from("/tmp/robco.plist")
                ],
            ]
        );
    }

    #[test]
    fn only_absent_service_bootout_error_is_tolerated() {
        assert!(service_is_absent(
            Some(3),
            b"Boot-out failed: 3: No such process"
        ));
        assert!(!service_is_absent(Some(3), b"Permission denied"));
        assert!(!service_is_absent(Some(5), b"Input/output error"));
    }
}

#[cfg(target_os = "macos")]
fn service_is_absent(status: Option<i32>, stderr: &[u8]) -> bool {
    status == Some(3) && String::from_utf8_lossy(stderr).contains("No such process")
}
