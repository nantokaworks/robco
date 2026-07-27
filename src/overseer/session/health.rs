//! What the daemon knows about whether a spawned session can authenticate.
//!
//! The complement to the credential channel: `gh auth status`, `docker info`,
//! and `tailscale status` all exist because a broken credential is otherwise
//! only visible as the work failing. This record is written by the start-up
//! preflight and by the first session that fails on authentication, and read
//! back by `robco overseer status`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::env::Credential;
use crate::{Result, overseer::statefile::atomic_replace};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SessionHealthState {
    /// A session spawned by the daemon authenticated and produced a result.
    Ok,
    /// A session was refused by the provider — the credential, not the work.
    AuthFailed,
    /// The preflight did not run, or could not reach a verdict.
    Unknown,
}

impl SessionHealthState {
    fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::AuthFailed => "failed",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SessionHealth {
    pub(crate) at: DateTime<Utc>,
    pub(crate) state: SessionHealthState,
    /// Which environment name supplied the credential, and from which layer of
    /// the resolution order. Absent means no configured channel and nothing
    /// recognisable inherited — which is itself the likeliest explanation for
    /// an `AuthFailed` state under launchd.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) credential: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<String>,
}

impl SessionHealth {
    pub(crate) fn new(state: SessionHealthState, credential: Option<Credential>) -> Self {
        Self {
            at: Utc::now(),
            state,
            credential: credential.as_ref().map(|found| found.name.clone()),
            source: credential.as_ref().map(|found| found.source.label().into()),
            detail: None,
        }
    }

    pub(crate) fn with_detail(mut self, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        self.detail = (!detail.is_empty()).then_some(detail);
        self
    }

    pub(crate) fn load() -> Result<Option<Self>> {
        let path = crate::overseer::session_health_path()?;
        match std::fs::read(&path) {
            Ok(raw) => Ok(serde_json::from_slice(&raw).ok()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub(crate) fn save(&self) -> Result<()> {
        let path = crate::overseer::session_health_path()?;
        atomic_replace(&path, &serde_json::to_vec_pretty(self)?)
    }

    /// Record an authentication failure observed by a live session. Best-effort
    /// on purpose: this runs inside a session worker thread whose job is to
    /// return a verdict, and a health file that could not be written must not
    /// turn into a second, unrelated failure.
    pub(crate) fn record_auth_failure(detail: &str, credential: Option<Credential>) {
        let _ = Self::new(SessionHealthState::AuthFailed, credential)
            .with_detail(detail)
            .save();
    }

    /// The `robco overseer status` line. Reads as a sentence rather than a
    /// field dump because it sits among warnings an operator scans.
    pub(crate) fn summary(&self) -> String {
        let channel = match (&self.credential, &self.source) {
            (Some(name), Some(source)) => format!("{name} via {source}"),
            (Some(name), None) => name.clone(),
            _ => "no credential configured".into(),
        };
        let age = Utc::now()
            .signed_duration_since(self.at)
            .num_minutes()
            .max(0);
        let detail = self
            .detail
            .as_ref()
            .map(|detail| format!(" — {detail}"))
            .unwrap_or_default();
        format!(
            "session auth: {} ({channel}, checked {age}m ago){detail}",
            self.state.label()
        )
    }

    /// Shown under the status line when the channel is the likely cause, so the
    /// operator is not left to infer the fix from a state word.
    pub(crate) fn warning(&self) -> Option<&'static str> {
        (self.state == SessionHealthState::AuthFailed).then_some(SESSION_AUTH_HINT)
    }
}

/// Named here rather than at the call site because the CLI, the docs, and the
/// preflight all have to describe the same recovery.
pub(crate) const SESSION_AUTH_HINT: &str = "daemon-spawned sessions cannot authenticate — a launchd agent has no access to the interactive login session's keychain. Give it a non-interactive credential: run `claude setup-token`, put `CLAUDE_CODE_OAUTH_TOKEN=<token>` in `~/.robco/env` (or `overseer.session_env` in `~/.robco/config.json`), then reinstall the service with `robco overseer install-service` and reload it.";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overseer::session::env::EnvSource;

    #[test]
    fn summary_names_the_channel_and_the_state() {
        let health = SessionHealth::new(
            SessionHealthState::Ok,
            Some(Credential {
                name: "CLAUDE_CODE_OAUTH_TOKEN".into(),
                source: EnvSource::File,
            }),
        );

        let summary = health.summary();
        assert!(
            summary.starts_with("session auth: ok (CLAUDE_CODE_OAUTH_TOKEN via session env file")
        );
        assert!(health.warning().is_none());
    }

    #[test]
    fn failed_summary_carries_the_detail_and_the_recovery_hint() {
        let health = SessionHealth::new(SessionHealthState::AuthFailed, None)
            .with_detail("Failed to authenticate: OAuth session expired");

        let summary = health.summary();
        assert!(summary.contains("session auth: failed (no credential configured"));
        assert!(summary.ends_with("— Failed to authenticate: OAuth session expired"));
        assert_eq!(health.warning(), Some(SESSION_AUTH_HINT));
    }

    #[test]
    fn empty_detail_is_dropped_rather_than_rendered() {
        let health = SessionHealth::new(SessionHealthState::Unknown, None).with_detail("");

        assert!(health.detail.is_none());
        assert!(health.summary().ends_with("m ago)"));
        assert!(health.summary().starts_with("session auth: unknown"));
    }

    #[test]
    fn record_round_trips_through_json() {
        let health = SessionHealth::new(
            SessionHealthState::AuthFailed,
            Some(Credential {
                name: "ANTHROPIC_API_KEY".into(),
                source: EnvSource::Config,
            }),
        )
        .with_detail("invalid api key");

        let raw = serde_json::to_vec(&health).unwrap();
        assert_eq!(
            serde_json::from_slice::<SessionHealth>(&raw).unwrap(),
            health
        );
    }
}
