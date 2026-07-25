#[cfg(target_os = "macos")]
use std::{ffi::OsString, path::Path, process::Output, time::Duration};

#[cfg(target_os = "macos")]
use crate::{Error, Result};

#[cfg(target_os = "macos")]
const LABEL: &str = "com.robco.overseer";

/// How long the load sequence is willing to wait for launchd to catch up.
///
/// `launchctl bootout` is asynchronous: it returns as soon as launchd accepts
/// the teardown request, not when the job has actually left the domain. The
/// Overseer daemon is `KeepAlive` and long-running, so it can still hold the
/// label when the following `bootstrap` lands, and launchd answers that with
/// `EIO`. Both the settle poll and the bootstrap retry are attempt-capped so
/// the wizard can never hang on a service that refuses to exit.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
pub(super) struct SettleBudget {
    pub(super) polls: u32,
    pub(super) poll_interval: Duration,
    pub(super) bootstrap_attempts: u32,
    pub(super) retry_interval: Duration,
}

#[cfg(target_os = "macos")]
impl SettleBudget {
    pub(super) const fn launchd() -> Self {
        Self {
            polls: 20,
            poll_interval: Duration::from_millis(250),
            bootstrap_attempts: 5,
            retry_interval: Duration::from_millis(500),
        }
    }
}

/// The launchd domain the wizard is loading the Overseer plist into, plus the
/// budget that bounds the wait for the domain to become available.
#[cfg(target_os = "macos")]
pub(super) struct Domain<'a> {
    pub(super) name: &'a str,
    pub(super) plist: &'a Path,
    pub(super) budget: SettleBudget,
}

#[cfg(target_os = "macos")]
impl Domain<'_> {
    pub(super) fn service(&self) -> String {
        format!("{}/{LABEL}", self.name)
    }

    /// Poll the domain until the booted-out label is genuinely gone, so the
    /// bootstrap that follows is not racing the daemon's own shutdown.
    pub(super) fn wait_for_bootout<F>(&self, run: &mut F) -> Result<()>
    where
        F: FnMut(&[OsString]) -> std::io::Result<Output>,
    {
        let service = self.service();
        for attempt in 0..self.budget.polls {
            if attempt > 0 {
                std::thread::sleep(self.budget.poll_interval);
            }
            // Any non-zero exit means launchd no longer prints the label, which
            // is the only signal that the teardown has actually landed.
            let result = run(&["print".into(), service.as_str().into()])?;
            if !result.status.success() {
                return Ok(());
            }
        }
        Err(Error::Wizard(self.stuck(&format!(
            "launchd still lists {service} after bootout"
        ))))
    }

    /// Bootstrap the plist, retrying while launchd reports the domain is still
    /// busy with the outgoing job.
    pub(super) fn bootstrap<F>(&self, run: &mut F) -> Result<()>
    where
        F: FnMut(&[OsString]) -> std::io::Result<Output>,
    {
        let mut stderr = String::new();
        for attempt in 0..self.budget.bootstrap_attempts {
            if attempt > 0 {
                std::thread::sleep(self.budget.retry_interval);
            }
            let result = run(&[
                "bootstrap".into(),
                self.name.into(),
                self.plist.as_os_str().to_os_string(),
            ])?;
            if result.status.success() {
                return Ok(());
            }
            stderr = String::from_utf8_lossy(&result.stderr).trim().to_string();
            if !domain_is_busy(result.status.code(), &result.stderr) {
                return Err(Error::Wizard(format!(
                    "launchctl bootstrap failed: {stderr}"
                )));
            }
        }
        Err(Error::Wizard(
            self.stuck(&format!("launchctl bootstrap failed: {stderr}")),
        ))
    }

    /// A budget-exhausted failure names the state and the manual recovery, so
    /// the operator is not left holding a bare `launchctl` errno.
    fn stuck(&self, detail: &str) -> String {
        format!(
            "{detail}. The previous service is still shutting down; once it exits, run: launchctl bootstrap {} {}",
            self.name,
            self.plist.display()
        )
    }
}

/// launchd answers `EIO` when the domain still carries the label being
/// bootstrapped into. That is transient by nature — the job is on its way out —
/// so it is worth retrying, unlike every other bootstrap failure.
#[cfg(target_os = "macos")]
fn domain_is_busy(status: Option<i32>, stderr: &[u8]) -> bool {
    status == Some(5) && String::from_utf8_lossy(stderr).contains("Input/output error")
}
