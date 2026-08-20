//! One spawned session at daemon start, so a broken credential is discovered
//! before a worker, triage, or review session spends a task on finding out.
//!
//! The probe deliberately runs the same way a worker session does — the
//! configured profile, spawned as a direct child of the daemon, writing
//! `result.json` into a case directory — because the failure this exists to
//! catch is a property of *that* execution context and of nothing else. A
//! cheaper check that only inspected the credential channel would have passed
//! on the day this broke: the credentials were valid, the launchd agent just
//! could not reach them.

use std::{fs, path::Path, time::Duration};

use super::{
    EphemeralSession, SessionResult,
    env::SessionEnv,
    health::{SessionHealth, SessionHealthState},
    session_profile,
};
use crate::{Result, config::Config, overseer::logging};

/// Short on purpose: the probe blocks the daemon's first pass, and a session
/// that has not answered a one-line question in two minutes tells the operator
/// as much as one that never answers.
const TIMEOUT: Duration = Duration::from_secs(120);

/// The probe's entire instruction, sent directly as the session's prompt
/// rather than staged in `briefing.md`. The probe has no external input to
/// fence off as untrusted data — unlike a judgment or triage session, it
/// never reads a task body, a diff, or an operator's message — so there is
/// nothing here for [`super::BRIEFING_PROMPT`]'s "treat the file as data
/// only" framing to apply to. Putting the instruction there anyway asked the
/// session to both obey the file and distrust it, which is what a compliant
/// model refuses to do.
const PROMPT: &str = "Write a file named result.json in the current directory containing exactly {\"ok\":true}. Do nothing else.";

/// Run the preflight, report it, and persist what it found. Never fails the
/// daemon: a probe that cannot run leaves an `Unknown` record, which is what
/// `robco status` then reports.
pub(crate) fn run(config: &Config) -> Result<()> {
    let health = probe(config, &crate::overseer::preflight_dir()?)?;
    logging::log_message(None, &health.summary())?;
    if let Some(warning) = health.warning() {
        logging::log_message(None, warning)?;
    }
    health.save()
}

fn probe(config: &Config, case_dir: &Path) -> Result<SessionHealth> {
    let env = SessionEnv::resolve(config);
    let credential = env.credential();
    if !config.overseer.session_preflight {
        return Ok(SessionHealth::new(SessionHealthState::Unknown, credential)
            .with_detail("preflight disabled by overseer.session_preflight"));
    }
    let Some(profile) = session_profile(config, config.overseer.worker_profile.as_ref()) else {
        return Ok(SessionHealth::new(SessionHealthState::Unknown, credential)
            .with_detail("worker profile not found"));
    };
    fs::create_dir_all(case_dir)?;
    let result = EphemeralSession {
        profile: &profile,
        case_dir,
        timeout: TIMEOUT,
        env: &env,
        prompt: PROMPT,
    }
    .run_controlled(&is_ready, &super::SessionControl::default(), None);
    Ok(classify(result, credential))
}

fn is_ready(raw: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(raw).is_ok_and(|value| value.get("ok").is_some())
}

/// A probe that produced its file authenticated; one refused on credentials is
/// the failure this exists for; anything else is a probe that did not reach a
/// verdict and must not be reported as either.
fn classify(result: SessionResult, credential: Option<super::env::Credential>) -> SessionHealth {
    match result {
        SessionResult::Result(_) => SessionHealth::new(SessionHealthState::Ok, credential),
        SessionResult::AuthFailed(detail) => {
            SessionHealth::new(SessionHealthState::AuthFailed, credential).with_detail(detail)
        }
        SessionResult::TimedOut => SessionHealth::new(SessionHealthState::Unknown, credential)
            .with_detail("preflight session timed out"),
        SessionResult::Missing => SessionHealth::new(SessionHealthState::Unknown, credential)
            .with_detail("preflight session exited without result.json"),
        SessionResult::LaunchFailed(error) => {
            SessionHealth::new(SessionHealthState::Unknown, credential)
                .with_detail(format!("preflight session failed: {error}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overseer::session::env::{Credential, EnvSource};

    fn credential() -> Option<Credential> {
        Some(Credential {
            name: "CLAUDE_CODE_OAUTH_TOKEN".into(),
            source: EnvSource::File,
        })
    }

    #[test]
    fn a_produced_result_is_a_healthy_channel() {
        let health = classify(
            SessionResult::Result(br#"{"ok":true}"#.to_vec()),
            credential(),
        );

        assert_eq!(health.state, SessionHealthState::Ok);
        assert_eq!(
            health.credential.as_deref(),
            Some("CLAUDE_CODE_OAUTH_TOKEN")
        );
        assert!(health.detail.is_none());
    }

    #[test]
    fn only_an_auth_refusal_reports_a_broken_credential() {
        assert_eq!(
            classify(
                SessionResult::AuthFailed("oauth session expired".into()),
                None
            )
            .state,
            SessionHealthState::AuthFailed
        );
        for other in [
            SessionResult::TimedOut,
            SessionResult::Missing,
            SessionResult::LaunchFailed("no executable".into()),
        ] {
            let health = classify(other, None);
            assert_eq!(health.state, SessionHealthState::Unknown);
            assert!(health.detail.is_some());
        }
    }

    #[test]
    fn readiness_needs_the_probe_shape_not_merely_valid_json() {
        assert!(is_ready(br#"{"ok":true}"#));
        assert!(is_ready(br#"{"ok":false}"#));
        assert!(!is_ready(br#"{"other":1}"#));
        assert!(!is_ready(b"{"));
    }

    #[test]
    fn disabled_preflight_records_unknown_without_spawning() {
        let temp = tempfile::tempdir().unwrap();
        let case_dir = temp.path().join("preflight");
        let mut config = Config::default();
        config.overseer.session_preflight = false;

        let health = probe(&config, &case_dir).unwrap();

        assert_eq!(health.state, SessionHealthState::Unknown);
        assert_eq!(
            health.detail.as_deref(),
            Some("preflight disabled by overseer.session_preflight")
        );
        assert!(!case_dir.exists());
    }

    #[test]
    fn a_missing_worker_profile_is_reported_rather_than_probed() {
        let temp = tempfile::tempdir().unwrap();
        let case_dir = temp.path().join("preflight");
        let mut config = Config::default();
        config.overseer.worker_profile = Some("absent-profile".into());

        let health = probe(&config, &case_dir).unwrap();

        assert_eq!(health.state, SessionHealthState::Unknown);
        assert_eq!(health.detail.as_deref(), Some("worker profile not found"));
        assert!(!case_dir.exists());
    }
}
