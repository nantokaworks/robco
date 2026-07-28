//! Delivery of daemon-decision notifications: draining the decision cursor,
//! coalescing escalation runs into a digest, and — see `localize.rs` —
//! optionally translating the result before it reaches Discord. Split out of
//! `gateway.rs`, which keeps the connection loop and message-command
//! routing.

use super::{
    cursor::{DecisionCursor, PendingDecision},
    localize::{self, LocalizeOutcome, LocalizeSpawner, TitleCache},
    notifications::{Notification, digest, from_decision},
    ops_agent::PendingSession,
};
use crate::overseer::{config::DiscordConfig, logging::DecisionKind};
use serde_json::json;
use std::{collections::VecDeque, time::Duration};
use twilight_http::Client;
use twilight_model::{
    channel::message::embed::Embed,
    id::{Id, marker::ChannelMarker},
};

/// A localization session started on an earlier tick, tracked across ticks
/// exactly like `retry_at`/`retry_delay` — in-process state only. A restart
/// mid-flight loses it harmlessly: the cursor never advanced past
/// `completed`, so the next run re-reads the same entry from disk and starts
/// localizing it again.
pub(super) struct InFlight {
    session: Box<dyn PendingSession>,
    /// The English rendering, delivered if localization fails.
    fallback: Notification,
    language: String,
    completed: PendingDecision,
}

/// Resolves an in-flight localization first (it takes priority: the cursor
/// has not advanced past it, so re-reading the log would only hand back the
/// same entry alongside whatever else has since arrived), otherwise pulls a
/// fresh batch off the decision cursor and starts localizing/sending each
/// entry in turn. Mutates `retry_at`/`retry_delay` exactly as the inline tick
/// loop used to, so a send failure still backs off the same way.
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
    if let Some(mut flight) = in_flight.take() {
        if let Some(result) = flight.session.poll() {
            let notification =
                localize::resolve(localize_cache, &flight.language, &flight.fallback, result);
            let delivered = match channel_id(current) {
                Some(channel) => send_embed(http, channel, notification).await,
                None => false,
            };
            if delivered {
                cursor
                    .complete(flight.completed, true)
                    .map_err(|error| error.to_string())?;
                *retry_delay = Duration::from_secs(1);
            } else {
                *retry_at = tokio::time::Instant::now() + *retry_delay;
                *retry_delay = (*retry_delay * 2).min(Duration::from_secs(30));
                *in_flight = Some(flight);
            }
        } else {
            *in_flight = Some(flight);
        }
        return Ok(());
    }

    let pending = cursor.next_batch(500).map_err(|error| error.to_string())?;
    let mut pending = bounded_batch(pending, 20);
    let language = active_language(current);
    while !pending.is_empty() {
        let (count, notification) = next_notification(current, &pending);
        let mut completed = None;
        for _ in 0..count {
            completed = pending.pop_front();
        }
        let completed = completed.expect("planned pending decision");
        let Some(notification) = notification else {
            cursor
                .complete(completed, true)
                .map_err(|error| error.to_string())?;
            continue;
        };
        let fallback = notification.clone();
        match localize::start(
            localize_spawner,
            localize_cache,
            language.as_deref(),
            notification,
        ) {
            LocalizeOutcome::Ready(notification) => {
                let delivered = match channel_id(current) {
                    Some(channel) => send_embed(http, channel, notification).await,
                    None => false,
                };
                if !delivered {
                    *retry_at = tokio::time::Instant::now() + *retry_delay;
                    *retry_delay = (*retry_delay * 2).min(Duration::from_secs(30));
                    break;
                }
                cursor
                    .complete(completed, true)
                    .map_err(|error| error.to_string())?;
                *retry_delay = Duration::from_secs(1);
            }
            LocalizeOutcome::Pending(session) => {
                *in_flight = Some(InFlight {
                    session,
                    fallback,
                    language: language.clone().expect("pending implies a language"),
                    completed,
                });
                break;
            }
        }
    }
    Ok(())
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

pub(super) fn bounded_batch(
    pending: Vec<PendingDecision>,
    limit: usize,
) -> VecDeque<PendingDecision> {
    let mut end = pending.len().min(limit);
    if end == 0 {
        return VecDeque::new();
    }
    while end < pending.len()
        && pending[end - 1].entry.as_ref().is_some_and(is_digest_entry)
        && pending[end].entry.as_ref().is_some_and(is_digest_entry)
    {
        end += 1;
    }
    pending.into_iter().take(end).collect()
}

pub(super) fn next_notification(
    config: &DiscordConfig,
    pending: &VecDeque<PendingDecision>,
) -> (usize, Option<Notification>) {
    let first = pending.front().expect("non-empty pending decisions");
    if first.entry.as_ref().is_some_and(is_digest_entry) {
        let entries = pending
            .iter()
            .map(|item| item.entry.as_ref())
            .take_while(|entry| entry.is_some_and(is_digest_entry))
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        return (entries.len(), digest(config, &entries));
    }
    (
        1,
        first
            .entry
            .as_ref()
            .and_then(|entry| from_decision(config, entry)),
    )
}

fn is_digest_entry(entry: &crate::overseer::logging::DecisionEntry) -> bool {
    entry.source.as_deref() != Some("discord")
        && matches!(
            entry.kind,
            DecisionKind::Escalate | DecisionKind::CircuitOpen
        )
}

pub(super) fn channel_id(config: &DiscordConfig) -> Option<Id<ChannelMarker>> {
    let raw = config.channel_id.as_deref()?.parse::<u64>().ok()?;
    (raw != 0).then(|| Id::new(raw))
}

async fn send_embed(http: &Client, channel: Id<ChannelMarker>, notification: Notification) -> bool {
    let embed: Embed = match serde_json::from_value(json!({
        "title": notification.title,
        "description": notification.description,
        "color": notification.color,
        "type": "rich"
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
