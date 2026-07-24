mod briefing;
mod completion;
mod keys;
mod merge_gate;
mod queue;
mod result;
mod revisions;

pub(crate) use merge_gate::{change_facts, judgment_after_gate, merge_case};
pub use queue::JudgmentQueue;
pub use result::{DispatchAdvice, MergeJudgment};

use crate::config::{Config, Profile};
use crate::overseer::dispatch::Candidate;
use crate::overseer::session::{
    EphemeralSession, SessionControl, SessionHandle, SessionResult, session_profile,
    terminate_stale_session,
};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path, time::Duration};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MergeCase {
    pub task_id: String,
    pub repo: String,
    pub pr_url: String,
    pub head_sha: String,
    pub title: String,
    pub body: String,
    pub files: Vec<String>,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone)]
pub(super) enum Request {
    Dispatch {
        key: String,
        approved: Vec<Candidate>,
    },
    Merge {
        key: String,
        case: MergeCase,
    },
}

impl Request {
    pub(super) fn key(&self) -> &str {
        match self {
            Self::Dispatch { key, .. } | Self::Merge { key, .. } => key,
        }
    }
}

pub(super) fn spawn_session(config: &Config, request: Request, root: &Path) -> SessionHandle {
    let profile = match &request {
        Request::Dispatch { .. } => judge_profile(config),
        Request::Merge { .. } => merge_judge_profile(config),
    };
    let root = root.to_path_buf();
    let timeout = Duration::from_secs(config.overseer.triage_timeout_mins.saturating_mul(60));
    SessionHandle::spawn(move |control| run_session(profile, timeout, request, &root, &control))
}

fn run_session(
    profile: Option<Profile>,
    timeout: Duration,
    request: Request,
    root: &Path,
    control: &SessionControl,
) -> SessionResult {
    let case_dir = root.join(request.key());
    if let Err(error) = fs::create_dir_all(&case_dir) {
        return SessionResult::LaunchFailed(error.to_string());
    }
    let input = match &request {
        Request::Dispatch { approved, .. } => serde_json::to_vec_pretty(approved),
        Request::Merge { case, .. } => serde_json::to_vec_pretty(case),
    };
    if let Err(error) = input
        .map_err(std::io::Error::other)
        .and_then(|raw| fs::write(case_dir.join("case.json"), raw))
    {
        return SessionResult::LaunchFailed(error.to_string());
    }
    if let Err(error) = fs::write(case_dir.join("briefing.md"), briefing::render(&request)) {
        return SessionResult::LaunchFailed(error.to_string());
    }
    let Some(profile) = profile else {
        return SessionResult::LaunchFailed("judgment profile not found".into());
    };
    let pid_path = case_dir.join("session.pid");
    terminate_stale_session(&pid_path);
    EphemeralSession {
        profile: &profile,
        case_dir: &case_dir,
        timeout,
    }
    .run_controlled(&result::is_complete, control, Some(&pid_path))
}

pub(crate) fn judge_profile(config: &Config) -> Option<Profile> {
    session_profile(config, config.overseer.judge_profile.as_ref())
}

pub(crate) fn merge_judge_profile(config: &Config) -> Option<Profile> {
    session_profile(config, config.overseer.merge_judge_profile.as_ref())
}

#[cfg(test)]
#[path = "judge/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "judge/queue_tests.rs"]
mod queue_tests;
