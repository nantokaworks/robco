//! Spawning and running the reviewer model's ephemeral session.
//!
//! Split out of the parent module so the `ReviewPass` state machine and the
//! session-process mechanics stay separately readable.

use std::{fs, path::Path, time::Duration};

use crate::{
    config::{Config, Profile},
    overseer::session::{
        BRIEFING_PROMPT, EphemeralSession, SessionControl, SessionHandle, SessionResult,
        env::SessionEnv, session_profile, terminate_stale_session,
    },
};

use super::{briefing, digest::Digest, findings::Finding, result, rows::RowCase};

pub(super) fn spawn_session(
    config: &Config,
    digest: &Digest,
    findings: &[Finding],
    rows: &[RowCase],
    root: &Path,
) -> SessionHandle {
    let profile = review_profile(config);
    let timeout = Duration::from_secs(config.overseer.triage_timeout_mins.saturating_mul(60));
    let case_dir = root.join("session");
    let case = match serde_json::to_vec_pretty(digest) {
        Ok(raw) => raw,
        Err(error) => {
            return SessionHandle::spawn(move |_| SessionResult::LaunchFailed(error.to_string()));
        }
    };
    let prompt = briefing::render(digest, findings, rows, config.language.as_deref());
    let env = SessionEnv::resolve(config);
    SessionHandle::spawn(move |control| {
        run_session(profile, timeout, &case, &prompt, &case_dir, &control, &env)
    })
}

#[allow(clippy::too_many_arguments)]
fn run_session(
    profile: Option<Profile>,
    timeout: Duration,
    case: &[u8],
    prompt: &str,
    case_dir: &Path,
    control: &SessionControl,
    env: &SessionEnv,
) -> SessionResult {
    if let Err(error) = fs::create_dir_all(case_dir) {
        return SessionResult::LaunchFailed(error.to_string());
    }
    if let Err(error) = fs::write(case_dir.join("digest.json"), case)
        .and_then(|()| fs::write(case_dir.join("briefing.md"), prompt))
    {
        return SessionResult::LaunchFailed(error.to_string());
    }
    let Some(profile) = profile else {
        return SessionResult::LaunchFailed("review profile not found".into());
    };
    let pid_path = case_dir.join("session.pid");
    terminate_stale_session(&pid_path);
    EphemeralSession {
        profile: &profile,
        case_dir,
        timeout,
        env,
        prompt: BRIEFING_PROMPT,
    }
    .run_controlled(&result::is_complete, control, Some(&pid_path))
}

pub(crate) fn review_profile(config: &Config) -> Option<Profile> {
    config
        .overseer
        .review_profile
        .as_ref()
        .and_then(|name| session_profile(config, Some(name)))
}

pub(super) fn failed(result: SessionResult) -> String {
    match result {
        SessionResult::TimedOut => "session timed out".into(),
        SessionResult::Missing => "session exited without result.json".into(),
        SessionResult::AuthFailed(detail) => {
            format!("{}: {detail}", crate::overseer::session::auth::REASON)
        }
        SessionResult::LaunchFailed(error) => format!("session failed: {error}"),
        SessionResult::Result(_) => unreachable!("a result is not a failure"),
    }
}
