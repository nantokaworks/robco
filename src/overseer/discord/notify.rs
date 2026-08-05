//! Delivery of daemon-decision notifications: draining the decision cursor,
//! coalescing escalation runs into a digest, and — see `localize.rs` —
//! optionally translating the result before it reaches Discord. Split out of
//! `gateway.rs`, which keeps the connection loop and message-command
//! routing; batch planning lives in `notify_plan.rs`.

use super::{
    cursor::{DecisionCursor, PendingDecision},
    localize::{self, LocalizeOutcome, LocalizeSpawner, TitleCache},
    notifications::Notification,
    notify_plan::{bounded_batch, display_ids, next_notification},
    ops_agent::PendingSession,
    rollup::Planned,
};
use crate::overseer::config::DiscordConfig;
use serde_json::json;
use std::{collections::VecDeque, time::Duration};
use twilight_http::Client;
use twilight_model::{
    channel::message::embed::Embed,
    id::{Id, marker::ChannelMarker},
};

/// Undelivered notifications for one cursor advance, tracked across ticks —
/// in-process state only. A restart mid-flight loses it harmlessly: the
/// cursor never advanced past `completed`, so the next run re-reads the same
/// entries from disk and plans them again. One cursor advance can stand for
/// several notifications (a small escalation burst delivered individually),
/// so the whole queue is carried until every one is sent.
pub(super) struct InFlight {
    /// Localization session for the front of `queue`, started on an earlier
    /// tick. `None` while nothing is being localized.
    session: Option<Box<dyn PendingSession>>,
    /// A front notification already rendered (localized or English) whose
    /// send failed; retried before the queue moves on.
    rendered: Option<Notification>,
    /// English renderings not yet delivered; the front is the current one.
    queue: VecDeque<Notification>,
    language: Option<String>,
    completed: PendingDecision,
}

/// Resolves an in-flight queue first (it takes priority: the cursor has not
/// advanced past it, so re-reading the log would only hand back the same
/// entries alongside whatever else has since arrived), otherwise pulls a
/// fresh batch off the decision cursor and drives each planned notification
/// group in turn. Mutates `retry_at`/`retry_delay` exactly as the inline
/// tick loop used to, so a send failure still backs off the same way.
#[allow(clippy::too_many_arguments)]
pub(super) async fn deliver(
    http: &Client,
    current: &DiscordConfig,
    cursor: &mut DecisionCursor,
    localize_spawner: &mut dyn LocalizeSpawner,
    localize_cache: &mut TitleCache,
    in_flight: &mut Option<InFlight>,
    retry_at: &mut tokio::time::Instant,
    retry_delay: &mut Duration,
) -> Result<(), String> {
    if let Some(flight) = in_flight.take() {
        drive(
            http,
            current,
            cursor,
            localize_spawner,
            localize_cache,
            in_flight,
            retry_at,
            retry_delay,
            flight,
        )
        .await?;
        return Ok(());
    }

    let pending = cursor.next_batch(500).map_err(|error| error.to_string())?;
    let mut pending = bounded_batch(pending, 20);
    let display_ids = display_ids(&pending);
    let language = active_language(current);
    let now = chrono::Utc::now();
    while !pending.is_empty() {
        let (count, notifications) = match next_notification(current, &pending, &display_ids, now) {
            Planned::Consume {
                count,
                notifications,
            } => (count, notifications),
            // Held merges stay on the cursor; a later tick replans them.
            Planned::Hold => break,
        };
        let mut completed = None;
        for _ in 0..count {
            completed = pending.pop_front();
        }
        let completed = completed.expect("planned pending decision");
        let flight = InFlight {
            session: None,
            rendered: None,
            queue: notifications.into(),
            language: language.clone(),
            completed,
        };
        let drained = drive(
            http,
            current,
            cursor,
            localize_spawner,
            localize_cache,
            in_flight,
            retry_at,
            retry_delay,
            flight,
        )
        .await?;
        if !drained {
            break;
        }
    }
    Ok(())
}

/// Pushes one in-flight queue as far as it goes this tick: poll a running
/// localization, localize and send each remaining notification, advance the
/// cursor once the queue is empty. Returns whether the queue fully drained;
/// otherwise the flight is parked in `in_flight` (and, on a send failure,
/// the retry backoff is armed) for a later tick.
#[allow(clippy::too_many_arguments)]
async fn drive(
    http: &Client,
    current: &DiscordConfig,
    cursor: &mut DecisionCursor,
    localize_spawner: &mut dyn LocalizeSpawner,
    localize_cache: &mut TitleCache,
    in_flight: &mut Option<InFlight>,
    retry_at: &mut tokio::time::Instant,
    retry_delay: &mut Duration,
    mut flight: InFlight,
) -> Result<bool, String> {
    if let Some(mut session) = flight.session.take() {
        let Some(result) = session.poll() else {
            flight.session = Some(session);
            *in_flight = Some(flight);
            return Ok(false);
        };
        let fallback = flight.queue.pop_front().expect("session implies a front");
        let language = flight
            .language
            .as_deref()
            .expect("session implies a language");
        flight.rendered = Some(localize::resolve(
            localize_cache,
            language,
            &fallback,
            result,
        ));
    }
    loop {
        let notification = match flight.rendered.take() {
            Some(notification) => notification,
            None => {
                let Some(front) = flight.queue.front().cloned() else {
                    cursor
                        .complete(flight.completed, true)
                        .map_err(|error| error.to_string())?;
                    *retry_delay = Duration::from_secs(1);
                    return Ok(true);
                };
                match localize::start(
                    localize_spawner,
                    localize_cache,
                    flight.language.as_deref(),
                    front,
                ) {
                    LocalizeOutcome::Ready(notification) => {
                        flight.queue.pop_front();
                        notification
                    }
                    LocalizeOutcome::Pending(session) => {
                        flight.session = Some(session);
                        *in_flight = Some(flight);
                        return Ok(false);
                    }
                }
            }
        };
        let delivered = match report_channel_id(current) {
            Some(channel) => send_embed(http, channel, notification.clone()).await,
            None => false,
        };
        if !delivered {
            flight.rendered = Some(notification);
            *retry_at = tokio::time::Instant::now() + *retry_delay;
            *retry_delay = (*retry_delay * 2).min(Duration::from_secs(30));
            *in_flight = Some(flight);
            return Ok(false);
        }
    }
}

/// The language to localize notifications into, or `None` when the pass
/// should be skipped outright — `notify_localize` is off, or `language` is
/// unset/blank per the same rule `language_directive` itself applies. Reads
/// the top-level `Config` fresh each tick, mirroring how session spawners
/// elsewhere in this module (`ops_session::SystemSessionSpawner`) already
/// load it per request rather than threading it through `gateway::run`'s
/// signature.
fn active_language(discord: &DiscordConfig) -> Option<String> {
    if !discord.notify_localize {
        return None;
    }
    let language = crate::config::Config::load().ok()?.language?;
    (!crate::config::language_directive(Some(&language)).is_empty()).then_some(language)
}

pub(super) fn channel_id(config: &DiscordConfig) -> Option<Id<ChannelMarker>> {
    parse_channel(config.channel_id.as_deref())
}

/// Where reports — decision notifications and digests — are delivered:
/// `notify_channel_id` when it parses, otherwise `channel_id`. The fallback
/// covers both the unset field (a config written before it existed keeps its
/// single-channel behavior) and an unparseable value (a typo degrades to the
/// old routing rather than silencing reports). Only this module's `deliver`
/// reads it; chat and escalation-thread routing stay on `channel_id`.
pub(super) fn report_channel_id(config: &DiscordConfig) -> Option<Id<ChannelMarker>> {
    parse_channel(config.notify_channel_id.as_deref()).or_else(|| channel_id(config))
}

fn parse_channel(raw: Option<&str>) -> Option<Id<ChannelMarker>> {
    let raw = raw?.parse::<u64>().ok()?;
    (raw != 0).then(|| Id::new(raw))
}

async fn send_embed(http: &Client, channel: Id<ChannelMarker>, notification: Notification) -> bool {
    let fields = notification
        .fields
        .iter()
        .map(|field| json!({"name": field.name, "value": field.value, "inline": true}))
        .collect::<Vec<_>>();
    let embed: Embed = match serde_json::from_value(json!({
        "title": notification.title,
        "description": notification.description,
        "color": notification.color,
        "type": "rich",
        "fields": fields
    })) {
        Ok(embed) => embed,
        Err(error) => {
            eprintln!("overseer: Discord embed construction failed: {error}");
            return false;
        }
    };
    match http.create_message(channel).embeds(&[embed]).await {
        Ok(_) => true,
        Err(error) => {
            eprintln!("overseer: Discord notification failed: {error}");
            false
        }
    }
}

#[cfg(test)]
#[path = "notify_tests.rs"]
mod tests;
