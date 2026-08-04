mod actions;
mod category;
pub mod commands;
mod cursor;
mod gateway;
pub mod handler;
mod humanize;
pub(crate) mod ledger_requests;
mod localize;
mod message_split;
mod notifications;
mod notify;
mod ops_agent;
mod ops_effect;
mod ops_gateway;
mod ops_messages;
mod ops_result;
mod ops_session;
mod ops_sessions;
mod ops_state;
mod reactions;
mod respond;
mod task_create;
mod typing;

use crate::overseer::config::DiscordConfig;
use std::{
    path::Path,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub struct BotGuard {
    stop: Arc<AtomicBool>,
    config: Arc<RwLock<DiscordConfig>>,
    thread: Option<JoinHandle<()>>,
}

impl BotGuard {
    pub fn update_config(&self, config: DiscordConfig) {
        *self.config.write().unwrap_or_else(|lock| lock.into_inner()) = config;
    }
}

impl Drop for BotGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub fn start(
    config: DiscordConfig,
    env_file: Option<&Path>,
    ledger_requests: std::sync::mpsc::Sender<ledger_requests::LedgerRequest>,
) -> Result<BotGuard, String> {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let token = resolve_token(&config.token_env, env_file, |name| std::env::var(name).ok())
        .ok_or_else(|| format!("token environment variable {} is not set", config.token_env))?;
    validate_channels(&config)?;
    if config.allowed_user_ids.is_empty() {
        return Err("Discord allowed_user_ids is empty".into());
    }
    let stop = Arc::new(AtomicBool::new(false));
    let shared_config = Arc::new(RwLock::new(config));
    let thread_stop = Arc::clone(&stop);
    let thread_config = Arc::clone(&shared_config);
    let thread = thread::Builder::new()
        .name("robco-discord".into())
        .spawn(move || supervisor(thread_config, token, ledger_requests, thread_stop))
        .map_err(|error| format!("failed to spawn Discord thread: {error}"))?;
    Ok(BotGuard {
        stop,
        config: shared_config,
        thread: Some(thread),
    })
}

fn supervisor(
    config: Arc<RwLock<DiscordConfig>>,
    token: String,
    ledger_requests: std::sync::mpsc::Sender<ledger_requests::LedgerRequest>,
    stop: Arc<AtomicBool>,
) {
    let mut backoff = Duration::from_secs(1);
    let initial = config
        .read()
        .unwrap_or_else(|lock| lock.into_inner())
        .clone();
    let mut handler = handler::Handler::new(
        initial.channel_id.unwrap_or_default(),
        initial.allowed_user_ids,
        initial.confirmation_ttl_secs,
        initial.action_limit_per_hour,
    );
    while !stop.load(Ordering::Acquire) {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| error.to_string())?
                .block_on(gateway::run(
                    &config,
                    token.clone(),
                    &stop,
                    &mut handler,
                    &ledger_requests,
                ))
        }));
        if stop.load(Ordering::Acquire) {
            break;
        }
        match result {
            Ok(Err(error)) => eprintln!("overseer: Discord bot stopped: {error}"),
            Ok(Ok(())) => eprintln!("overseer: Discord gateway closed; reconnecting"),
            Err(_) => eprintln!("overseer: Discord bot panicked; restarting"),
        }
        wait_backoff(backoff, &stop);
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

fn wait_backoff(duration: Duration, stop: &AtomicBool) {
    let deadline = std::time::Instant::now() + duration;
    while !stop.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(100));
    }
}

/// The gateway starts when at least one of `channel_id`, `notify_channel_id`,
/// or `chat_category_ids` gives it a channel to serve; requiring `channel_id`
/// alone would keep the whole bot down on a config that only names the other
/// two (dropr:390). A set-but-unparseable `channel_id` is still an error, as
/// is a set-but-unparseable `notify_channel_id` with no `channel_id` behind
/// it — in that case `notify::report_channel_id` has no fallback and every
/// report would be dropped silently. With `channel_id` present the typo'd
/// value keeps degrading to the old routing, exactly as that function
/// documents.
fn validate_channels(config: &DiscordConfig) -> Result<(), String> {
    let channel = match config.channel_id.as_deref() {
        Some(raw) if !valid_channel(raw) => return Err("Discord channel_id is invalid".into()),
        raw => raw.is_some(),
    };
    let notify = match config.notify_channel_id.as_deref() {
        Some(raw) if !valid_channel(raw) && !channel => {
            return Err("Discord notify_channel_id is invalid".into());
        }
        Some(raw) => valid_channel(raw),
        None => false,
    };
    if !channel && !notify && config.chat_category_ids.is_empty() {
        return Err(
            "no Discord channel is configured (set channel_id, notify_channel_id, or chat_category_ids)"
                .into(),
        );
    }
    Ok(())
}

fn valid_channel(raw: &str) -> bool {
    raw.parse::<u64>().is_ok_and(|id| id != 0)
}

/// Resolve the bot token: the process environment wins, exactly as it did
/// before the env file could carry this credential too; the file is
/// consulted only when the name is absent or blank there. `inherited` is
/// injected so the precedence is testable without touching a real process
/// environment.
fn resolve_token(
    token_env: &str,
    env_file: Option<&Path>,
    inherited: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    inherited(token_env)
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env_file
                .and_then(|path| crate::overseer::session::env::lookup_var(path, token_env))
                .filter(|value| !value.trim().is_empty())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_environment_wins_over_the_env_file() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("env");
        std::fs::write(&file, "ROBCO_DISCORD_TOKEN=from-file\n").unwrap();

        let token = resolve_token("ROBCO_DISCORD_TOKEN", Some(&file), |_| {
            Some("from-process".to_string())
        });

        assert_eq!(token.as_deref(), Some("from-process"));
    }

    #[test]
    fn the_env_file_is_used_when_the_process_does_not_carry_the_token() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("env");
        std::fs::write(&file, "ROBCO_DISCORD_TOKEN=from-file\n").unwrap();

        let token = resolve_token("ROBCO_DISCORD_TOKEN", Some(&file), |_| None);

        assert_eq!(token.as_deref(), Some("from-file"));
    }

    #[test]
    fn a_blank_process_value_falls_through_to_the_env_file() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("env");
        std::fs::write(&file, "ROBCO_DISCORD_TOKEN=from-file\n").unwrap();

        let token = resolve_token("ROBCO_DISCORD_TOKEN", Some(&file), |_| {
            Some("  ".to_string())
        });

        assert_eq!(token.as_deref(), Some("from-file"));
    }

    #[test]
    fn neither_source_carrying_the_token_is_reported_as_absent() {
        assert_eq!(resolve_token("ROBCO_DISCORD_TOKEN", None, |_| None), None);
    }

    fn channels(channel: Option<&str>, notify: Option<&str>, categories: &[&str]) -> DiscordConfig {
        DiscordConfig {
            channel_id: channel.map(String::from),
            notify_channel_id: notify.map(String::from),
            chat_category_ids: categories.iter().map(|id| id.to_string()).collect(),
            ..DiscordConfig::default()
        }
    }

    #[test]
    fn each_channel_source_alone_lets_the_gateway_start() {
        assert!(validate_channels(&channels(Some("42"), None, &[])).is_ok());
        assert!(validate_channels(&channels(None, Some("42"), &[])).is_ok());
        assert!(validate_channels(&channels(None, None, &["7"])).is_ok());
    }

    #[test]
    fn no_channel_source_at_all_is_an_error() {
        let error = validate_channels(&channels(None, None, &[])).unwrap_err();
        assert!(error.contains("no Discord channel is configured"), "{error}");
    }

    #[test]
    fn a_set_but_invalid_channel_id_is_still_an_error() {
        for raw in ["oops", "0", ""] {
            let error = validate_channels(&channels(Some(raw), Some("42"), &[])).unwrap_err();
            assert_eq!(error, "Discord channel_id is invalid");
        }
    }

    #[test]
    fn an_invalid_notify_channel_without_fallback_is_an_error() {
        let error = validate_channels(&channels(None, Some("oops"), &["7"])).unwrap_err();
        assert_eq!(error, "Discord notify_channel_id is invalid");
    }

    #[test]
    fn an_invalid_notify_channel_degrades_when_channel_id_backs_it() {
        assert!(validate_channels(&channels(Some("42"), Some("oops"), &[])).is_ok());
    }
}
