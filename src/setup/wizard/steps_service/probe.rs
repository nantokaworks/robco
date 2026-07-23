#[cfg(target_os = "macos")]
use std::{process::Command, time::Duration};

#[cfg(target_os = "macos")]
use crate::overseer::exec::run_timeout;

#[cfg(target_os = "macos")]
const LABEL: &str = "com.robco.overseer";

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ServiceState {
    NotInstalled,
    Unloaded,
    Loaded,
}

#[cfg(target_os = "macos")]
pub(super) struct ServiceProbe {
    pub(super) state: ServiceState,
    pub(super) uid: Option<String>,
}

#[cfg(target_os = "macos")]
pub(super) fn run() -> ServiceProbe {
    let Some(home) = dirs::home_dir() else {
        return unloaded(None);
    };
    let plist = home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{LABEL}.plist"));
    match plist.try_exists() {
        Ok(false) => {
            return ServiceProbe {
                state: ServiceState::NotInstalled,
                uid: None,
            };
        }
        Err(_) => return unloaded(None),
        Ok(true) => {}
    }
    let Ok(uid) = super::command_stdout("id", &["-u"]) else {
        return unloaded(None);
    };
    let state = state_with(&uid, launchctl_print);
    ServiceProbe {
        state,
        uid: Some(uid),
    }
}

#[cfg(target_os = "macos")]
fn state_with<F>(uid: &str, launchctl_print: F) -> ServiceState
where
    F: FnOnce(&str) -> std::io::Result<bool>,
{
    if launchctl_print(uid).unwrap_or(false) {
        ServiceState::Loaded
    } else {
        ServiceState::Unloaded
    }
}

#[cfg(target_os = "macos")]
fn launchctl_print(uid: &str) -> std::io::Result<bool> {
    let service = format!("gui/{uid}/{LABEL}");
    let mut command = Command::new("launchctl");
    command.args(["print", &service]);
    run_timeout(command, Duration::from_secs(5)).map(|output| output.status.success())
}

#[cfg(target_os = "macos")]
fn unloaded(uid: Option<String>) -> ServiceProbe {
    ServiceProbe {
        state: ServiceState::Unloaded,
        uid,
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::{ServiceState, state_with};

    #[test]
    fn launchctl_probe_failure_degrades_to_unloaded() {
        let state = state_with("501", |_| Err(std::io::Error::other("probe failed")));

        assert_eq!(state, ServiceState::Unloaded);
    }
}
