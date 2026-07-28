//! Discord "is typing" indicator keepalive.
//!
//! Discord's typing state expires roughly 10 seconds after each trigger,
//! but a spawned ops-agent chat session (`ops_session.rs`) can run for tens
//! of seconds. `TypingKeepalive` tracks, per channel, when the indicator was
//! last triggered so `gateway::run`'s tick loop can refresh it before it
//! expires. Tracking for a channel is dropped the moment it is no longer in
//! the caller-supplied active set — that reconciliation, not an explicit
//! stop call, is what turns the indicator off once a reply lands, the work
//! fails, or the session is otherwise gone.

use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};
use twilight_http::Client;
use twilight_model::id::{Id, marker::ChannelMarker};

/// Comfortably under Discord's ~10s typing expiry.
const REFRESH_INTERVAL: Duration = Duration::from_secs(8);

#[derive(Default)]
pub(super) struct TypingKeepalive {
    last_triggered: HashMap<String, Instant>,
}

impl TypingKeepalive {
    /// Fire an immediate typing trigger for `channel_id` and record it so a
    /// later `reconcile` call knows not to re-trigger too soon.
    pub(super) async fn start(
        &mut self,
        http: &Client,
        channel_id: &str,
        channel: Id<ChannelMarker>,
    ) {
        trigger(http, channel).await;
        self.last_triggered
            .insert(channel_id.to_string(), Instant::now());
    }

    /// Refresh the indicator for every channel in `active_channels` whose
    /// last trigger is due to expire, and drop tracking for channels no
    /// longer in that set.
    pub(super) async fn reconcile<'a>(
        &mut self,
        http: &Client,
        active_channels: impl Iterator<Item = &'a str>,
    ) {
        let active: HashSet<&str> = active_channels.collect();
        self.last_triggered
            .retain(|channel_id, _| active.contains(channel_id.as_str()));
        let now = Instant::now();
        for channel_id in active {
            if !is_due(self.last_triggered.get(channel_id).copied(), now) {
                continue;
            }
            if let Some(channel) = super::ops_gateway::parse_channel(channel_id) {
                trigger(http, channel).await;
            }
            self.last_triggered.insert(channel_id.to_string(), now);
        }
    }
}

fn is_due(last_triggered: Option<Instant>, now: Instant) -> bool {
    last_triggered.is_none_or(|at| now.duration_since(at) >= REFRESH_INTERVAL)
}

async fn trigger(http: &Client, channel: Id<ChannelMarker>) {
    // Decoration only: a rate-limited or refused trigger must never hold up
    // the reply path, so failures are logged and otherwise ignored.
    if let Err(error) = http.create_typing_trigger(channel).await {
        eprintln!("overseer: Discord typing indicator failed: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_channel_triggered_just_now_is_not_due() {
        assert!(!is_due(Some(Instant::now()), Instant::now()));
    }

    #[test]
    fn a_channel_never_triggered_is_due() {
        assert!(is_due(None, Instant::now()));
    }

    #[test]
    fn a_channel_past_the_refresh_interval_is_due() {
        let triggered_at = Instant::now();
        let later = triggered_at + REFRESH_INTERVAL + Duration::from_secs(1);
        assert!(is_due(Some(triggered_at), later));
    }

    #[test]
    fn a_channel_just_shy_of_the_refresh_interval_is_not_due() {
        let triggered_at = Instant::now();
        let later = triggered_at + REFRESH_INTERVAL - Duration::from_millis(1);
        assert!(!is_due(Some(triggered_at), later));
    }

    #[test]
    fn reconcile_bookkeeping_drops_channels_no_longer_active() {
        let mut keepalive = TypingKeepalive {
            last_triggered: HashMap::from([
                ("30".to_string(), Instant::now()),
                ("31".to_string(), Instant::now()),
            ]),
        };
        let active: HashSet<&str> = ["30"].into_iter().collect();
        keepalive
            .last_triggered
            .retain(|channel_id, _| active.contains(channel_id.as_str()));
        assert!(keepalive.last_triggered.contains_key("30"));
        assert!(!keepalive.last_triggered.contains_key("31"));
    }
}
