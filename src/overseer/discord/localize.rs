//! Translates a rendered [`Notification`] into the operator's configured
//! `language` via an LLM pass, reusing the same ephemeral-session substrate
//! as the Discord ops agent (`ops_session.rs`).
//!
//! Design choice: the cache key is the *whole* rendered notification
//! (language, title, description), not the title alone. A title-only key
//! would risk serving a cached description — with another entry's task id,
//! repo, or PR url baked in — for a title that recurs with different data.
//! Keying on the full content only saves a spawn for a genuine repeat (the
//! same event redelivered after a failed send, or an identical digest), but
//! never serves a stale identifier.

use super::{notifications::Notification, ops_agent::PendingSession};
use crate::{
    config::Config,
    overseer::{
        session::{
            BRIEFING_PROMPT, EphemeralSession, SessionHandle, SessionResult, auth, env::SessionEnv,
        },
        triage::triage_profile,
    },
};
use nanoid::nanoid;
use serde::Deserialize;
use std::{collections::HashMap, fs, path::PathBuf, sync::mpsc::TryRecvError, time::Duration};

pub(super) trait LocalizeSpawner: Send {
    fn spawn(
        &mut self,
        language: &str,
        notification: &Notification,
    ) -> Result<Box<dyn PendingSession>, String>;
}

type CacheKey = (String, String, String);

#[derive(Default)]
pub(super) struct TitleCache(HashMap<CacheKey, (String, String)>);

impl TitleCache {
    fn key(language: &str, notification: &Notification) -> CacheKey {
        (
            language.to_string(),
            notification.title.clone(),
            notification.description.clone(),
        )
    }

    fn get(&self, language: &str, notification: &Notification) -> Option<Notification> {
        let (title, description) = self.0.get(&Self::key(language, notification))?;
        Some(Notification {
            title: title.clone(),
            description: description.clone(),
            color: notification.color,
        })
    }

    fn insert(&mut self, language: &str, source: &Notification, localized: &Notification) {
        self.0.insert(
            Self::key(language, source),
            (localized.title.clone(), localized.description.clone()),
        );
    }
}

pub(super) enum LocalizeOutcome {
    /// Nothing to spawn: a cache hit, or localization is off. Deliver now.
    Ready(Notification),
    /// A session is running; poll it on a later tick.
    Pending(Box<dyn PendingSession>),
}

/// Starts localizing `notification`, or resolves it immediately from cache.
/// Never spawns when `language` is `None` — that is the byte-identical,
/// no-session path the "language absent" acceptance criterion requires.
pub(super) fn start(
    spawner: &mut dyn LocalizeSpawner,
    cache: &TitleCache,
    language: Option<&str>,
    notification: Notification,
) -> LocalizeOutcome {
    let Some(language) = language else {
        return LocalizeOutcome::Ready(notification);
    };
    if let Some(cached) = cache.get(language, &notification) {
        return LocalizeOutcome::Ready(cached);
    }
    match spawner.spawn(language, &notification) {
        Ok(session) => LocalizeOutcome::Pending(session),
        // A session that could not even launch falls back to English exactly
        // like a session that timed out or returned garbage — see `resolve`.
        Err(_) => LocalizeOutcome::Ready(notification),
    }
}

/// Turns a finished session's raw result into a `Notification`, falling back
/// to `fallback` (the original English rendering) on any failure so a
/// localization problem never drops or delays a notification.
pub(super) fn resolve(
    cache: &mut TitleCache,
    language: &str,
    fallback: &Notification,
    result: Result<Vec<u8>, String>,
) -> Notification {
    let localized = result.and_then(|raw| parse(&raw, fallback.color));
    match localized {
        Ok(notification) => {
            cache.insert(language, fallback, &notification);
            notification
        }
        Err(_) => fallback.clone(),
    }
}

pub(super) struct SystemLocalizeSpawner {
    root: PathBuf,
}

impl SystemLocalizeSpawner {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl LocalizeSpawner for SystemLocalizeSpawner {
    fn spawn(
        &mut self,
        language: &str,
        notification: &Notification,
    ) -> Result<Box<dyn PendingSession>, String> {
        let config = Config::load().map_err(|error| error.to_string())?;
        let profile = triage_profile(&config).ok_or_else(|| "ops profile not found".to_string())?;
        let timeout = Duration::from_secs(config.overseer.triage_timeout_mins.saturating_mul(60));
        let case_dir = self
            .root
            .join(format!("{}-{}", chrono::Utc::now().timestamp(), nanoid!(8)));
        let env = SessionEnv::resolve(&config);
        let language = language.to_string();
        let notification = notification.clone();
        let handle = spawn_session(case_dir, profile, timeout, env, move || {
            briefing(&notification, &language)
        });
        Ok(Box::new(SystemPending(handle)))
    }
}

fn spawn_session(
    case_dir: PathBuf,
    profile: crate::config::Profile,
    timeout: Duration,
    env: SessionEnv,
    build_briefing: impl FnOnce() -> String + Send + 'static,
) -> SessionHandle {
    SessionHandle::spawn(move |control| {
        let briefing = build_briefing();
        if let Err(error) = fs::create_dir_all(&case_dir)
            .and_then(|()| fs::write(case_dir.join("briefing.md"), briefing))
        {
            return SessionResult::LaunchFailed(error.to_string());
        }
        EphemeralSession {
            profile: &profile,
            case_dir: &case_dir,
            timeout,
            env: &env,
            prompt: BRIEFING_PROMPT,
        }
        .run_controlled(&is_complete, &control, Some(&case_dir.join("session.pid")))
    })
}

struct SystemPending(SessionHandle);

impl PendingSession for SystemPending {
    fn poll(&mut self) -> Option<Result<Vec<u8>, String>> {
        match self.0.try_recv() {
            Ok(SessionResult::Result(raw)) => Some(Ok(raw)),
            Ok(SessionResult::TimedOut) => Some(Err("session timed out".into())),
            Ok(SessionResult::Missing) => Some(Err("session exited without result.json".into())),
            Ok(SessionResult::AuthFailed(detail)) => {
                Some(Err(format!("{}: {detail}", auth::REASON)))
            }
            Ok(SessionResult::LaunchFailed(error)) => Some(Err(error)),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("session thread disconnected".into())),
        }
    }
}

fn briefing(notification: &Notification, language: &str) -> String {
    format!(
        "# Discord notification localization\n\nTranslate the notification below. Write result.json as {{\"title\":\"...\",\"description\":\"...\"}}. No other keys, no commentary.\n\n{}{}{}",
        crate::config::language_directive(Some(language)),
        data("NOTIFICATION_TITLE", &notification.title),
        data("NOTIFICATION_BODY", &notification.description),
    )
}

fn data(label: &str, value: &str) -> String {
    let escaped = value.replace("<<<END_EXTERNAL_DATA>>>", "<<<END_EXTERNAL_DATA_ESCAPED>>>");
    format!("<<<EXTERNAL_DATA {label}>>>\n{escaped}\n<<<END_EXTERNAL_DATA>>>\n\n")
}

#[derive(Debug, Deserialize)]
struct RawLocalized {
    title: String,
    description: String,
}

fn is_complete(raw: &[u8]) -> bool {
    serde_json::from_slice::<RawLocalized>(raw).is_ok()
}

fn parse(raw: &[u8], color: u32) -> Result<Notification, String> {
    let result: RawLocalized = serde_json::from_slice(raw).map_err(|error| error.to_string())?;
    if result.title.trim().is_empty() || result.description.trim().is_empty() {
        return Err("localized title/description must not be blank".into());
    }
    Ok(Notification {
        title: result.title,
        description: result.description,
        color,
    })
}

#[cfg(test)]
#[path = "localize_tests.rs"]
mod tests;
