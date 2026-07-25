#[cfg(target_os = "macos")]
use std::io::Write;

#[cfg(target_os = "macos")]
use super::settle::{Domain, SettleBudget};
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
            SettleBudget::launchd(),
        )
    }

    fn apply_with<W, F, P>(
        self,
        output: &mut W,
        mut run: F,
        probe: P,
        budget: SettleBudget,
    ) -> Result<()>
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
        let domain = Domain {
            name: &self.domain,
            plist: &self.path,
            budget,
        };
        if matches!(self.mode, BootstrapMode::Reload) {
            let service = domain.service();
            let result = run(&["bootout".into(), service.as_str().into()])?;
            if !result.status.success() && !service_is_absent(result.status.code(), &result.stderr)
            {
                return Err(Error::Wizard(format!(
                    "launchctl bootout failed: {}",
                    String::from_utf8_lossy(&result.stderr).trim()
                )));
            }
            // `bootout` only means launchd accepted the teardown, so the label
            // has to be observed leaving the domain before anything is loaded
            // back into it.
            domain.wait_for_bootout(&mut run)?;
        }
        domain.bootstrap(&mut run)?;
        // `launchctl bootstrap` can exit 0 and still leave nothing running, so
        // the end state is re-probed for a plain load as well as for a reload:
        // a silent no-op here is exactly how a dead daemon survives a wizard run.
        if probe() != super::probe::ServiceState::Loaded {
            return Err(Error::Wizard(
                match self.mode {
                    BootstrapMode::Load => "launchd service did not come up after bootstrap",
                    BootstrapMode::Reload => "launchd service did not come back after reload",
                }
                .into(),
            ));
        }
        let verb = match self.mode {
            BootstrapMode::Load => "loaded",
            BootstrapMode::Reload => "reloaded",
        };
        writeln!(
            output,
            "▌ robco ▸ launchd ··········· {verb} ({})",
            self.executable.display()
        )?;
        Ok(())
    }
}

#[cfg(all(test, target_os = "macos"))]
#[path = "plan_tests.rs"]
mod tests;

#[cfg(target_os = "macos")]
fn service_is_absent(status: Option<i32>, stderr: &[u8]) -> bool {
    status == Some(3) && String::from_utf8_lossy(stderr).contains("No such process")
}
