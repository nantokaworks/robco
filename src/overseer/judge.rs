mod audit;
mod briefing;
mod completed;
mod completion;
mod keys;
mod merge_gate;
mod pending;
mod queue;
mod result;
mod revisions;
mod snapshot;

pub(crate) use merge_gate::{change_facts, judgment_after_gate, merge_case};
pub use queue::JudgmentQueue;
pub use result::{DispatchAdvice, MergeAdvice, MergeJudgment};

use crate::config::{Config, Profile};
use crate::overseer::dispatch::Candidate;
use crate::overseer::session::{
    BRIEFING_PROMPT, EphemeralSession, SessionControl, SessionHandle, SessionResult,
    env::SessionEnv, session_profile, terminate_stale_session,
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

/// One question put to a judgment session.
///
/// Serializable because the queue is durable: a daemon restart must not lose a
/// question that was waiting, nor the one that was in flight — see
/// [`pending::PendingQueue`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

    /// Operator-facing name for the queue snapshot. Keys are hashes, so they
    /// say nothing to a reader comparing the queue against `decisions.jsonl`;
    /// the task id is what both surfaces have in common.
    pub(super) fn label(&self) -> String {
        match self {
            Self::Dispatch { approved, .. } => format!("dispatch:{}", approved.len()),
            Self::Merge { case, .. } => format!("merge:{}", case.task_id),
        }
    }

    /// The concurrency slot this request occupies in `JudgmentQueue::active`.
    ///
    /// Dispatch requests all share one slot — a dispatch round asks about the
    /// whole board, not one repository, so there is nothing to key it by —
    /// while merge requests get one slot per repository. Two pull requests
    /// against the same repository therefore still serialize, because they
    /// share a slot, while two against different repositories occupy
    /// separate slots and run concurrently, bounded only by the queue's
    /// global `max_concurrent_judges` ceiling.
    pub(super) fn slot(&self) -> String {
        match self {
            Self::Dispatch { .. } => "dispatch".to_owned(),
            Self::Merge { case, .. } => format!("repo:{}", case.repo),
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
    // Read off the live config rather than carried on `Request`: that type is
    // serialized into the durable pending queue, so a queued question would
    // answer in whatever language was configured when it was enqueued.
    let language = config.language.clone();
    // Resolved here, on the daemon's live config, rather than inside the
    // session thread: the channel is configuration, and a session that read it
    // later could run under a value the operator has since replaced.
    let env = SessionEnv::resolve(config);
    SessionHandle::spawn(move |control| {
        run_session(profile, timeout, request, &root, &control, language, &env)
    })
}

#[allow(clippy::too_many_arguments)]
fn run_session(
    profile: Option<Profile>,
    timeout: Duration,
    request: Request,
    root: &Path,
    control: &SessionControl,
    language: Option<String>,
    env: &SessionEnv,
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
    let prompt = briefing::render(&request, language.as_deref());
    if let Err(error) = fs::write(case_dir.join("briefing.md"), prompt) {
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
        env,
        prompt: BRIEFING_PROMPT,
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
#[path = "judge/completion_tests.rs"]
mod completion_tests;
#[cfg(test)]
#[path = "judge/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "judge/queue_tests.rs"]
mod queue_tests;

#[cfg(test)]
#[path = "judge/queue_concurrency_tests.rs"]
mod queue_concurrency_tests;

#[cfg(test)]
#[path = "judge/pending_tests.rs"]
mod pending_tests;
