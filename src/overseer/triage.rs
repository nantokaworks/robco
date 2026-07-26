mod actions;
mod completion;
mod queue;
mod result;

pub use queue::ExceptionQueue;

use self::actions::{briefing, recent_capture};
#[cfg(test)]
use self::completion::apply_session_result_with;
use super::{
    ledger::LedgerEntry,
    monitor::Observations,
    session::{
        EphemeralSession, SessionControl, SessionHandle, SessionResult, terminate_stale_session,
    },
};
use crate::config::{Config, Profile};
use chrono::Utc;
use nanoid::nanoid;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path, time::Duration};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExceptionCase {
    pub id: String,
    pub kind: String,
    pub task_id: String,
    pub display_id: String,
    pub worker_id: String,
    pub repo: String,
    pub reason: String,
    pub task_state: String,
}

pub(super) fn case_from(
    entry: &LedgerEntry,
    kind: &str,
    reason: &str,
    observed: &Observations,
) -> ExceptionCase {
    let task_state = observed
        .tasks
        .iter()
        .find(|task| task.task_id == entry.task_id)
        .map(|task| task.state.clone())
        .unwrap_or_else(|| "unknown".into());
    ExceptionCase {
        id: format!("{}-{}", Utc::now().format("%Y%m%d%H%M%S"), nanoid!(8)),
        kind: kind.into(),
        task_id: entry.task_id.clone(),
        display_id: entry.display_id.clone(),
        worker_id: entry.agent_id.clone(),
        repo: entry.repo.clone(),
        reason: reason.into(),
        task_state,
    }
}

pub(super) fn spawn_session(config: &Config, case: &ExceptionCase, root: &Path) -> SessionHandle {
    let case = case.clone();
    let root = root.to_path_buf();
    let profile = triage_profile(config);
    let timeout = Duration::from_secs(config.overseer.triage_timeout_mins.saturating_mul(60));
    let language = config.language.clone();
    SessionHandle::spawn(move |control| {
        run_session(profile, timeout, &case, &root, &control, language)
    })
}

fn run_session(
    profile: Option<Profile>,
    timeout: Duration,
    case: &ExceptionCase,
    root: &Path,
    control: &SessionControl,
    language: Option<String>,
) -> SessionResult {
    let case_dir = root.join(&case.id);
    if let Err(error) = fs::create_dir_all(&case_dir) {
        return SessionResult::LaunchFailed(error.to_string());
    }
    if let Err(error) = serde_json::to_vec_pretty(case)
        .map_err(std::io::Error::other)
        .and_then(|raw| fs::write(case_dir.join("case.json"), raw))
    {
        return SessionResult::LaunchFailed(error.to_string());
    }
    let pid_path = case_dir.join("session.pid");
    terminate_stale_session(&pid_path);
    let capture = recent_capture(&case.worker_id);
    let prompt = briefing(case, &capture, language.as_deref());
    if let Err(error) = fs::write(case_dir.join("briefing.md"), prompt) {
        return SessionResult::LaunchFailed(error.to_string());
    }
    let Some(profile) = profile else {
        return SessionResult::LaunchFailed("triage profile not found".into());
    };
    let session = EphemeralSession {
        profile: &profile,
        case_dir: &case_dir,
        timeout,
    };
    session.run_controlled(&result::is_complete, control, Some(&pid_path))
}

pub(crate) fn triage_profile(config: &Config) -> Option<Profile> {
    let name = config
        .overseer
        .triage_profile
        .as_ref()
        .unwrap_or(&config.default_program);
    config
        .profiles
        .iter()
        .find(|profile| &profile.name == name)
        .cloned()
        .or_else(|| {
            (config.overseer.triage_profile.is_none()).then(|| Profile {
                name: name.clone(),
                program: config.default_program_command(),
                autonomous_args: Vec::new(),
                model: None,
                backend: None,
            })
        })
}

pub(crate) fn recent_worker_capture(worker_id: &str) -> String {
    actions::recent_capture(worker_id)
}

#[cfg(test)]
#[path = "triage/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "triage/briefing_tests.rs"]
mod briefing_tests;

#[cfg(test)]
#[path = "triage/recovery_tests.rs"]
mod recovery_tests;
