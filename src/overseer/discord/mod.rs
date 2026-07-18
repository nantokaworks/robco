mod actions;
pub mod commands;
mod cursor;
mod gateway;
pub mod handler;
pub(crate) mod ledger_requests;
mod notifications;
mod ops_agent;
mod ops_gateway;
mod ops_messages;
mod ops_result;
mod ops_session;
mod ops_state;

use crate::overseer::config::DiscordConfig;
use std::{
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
    ledger_requests: std::sync::mpsc::Sender<ledger_requests::LedgerRequest>,
) -> Result<BotGuard, String> {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let token = std::env::var(&config.token_env)
        .map_err(|_| format!("token environment variable {} is not set", config.token_env))?;
    let channel = config
        .channel_id
        .as_deref()
        .ok_or_else(|| "Discord channel_id is not configured".to_string())?
        .parse::<u64>()
        .map_err(|_| "Discord channel_id is invalid".to_string())?;
    if channel == 0 {
        return Err("Discord channel_id is invalid".into());
    }
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
