//! Time-window rollup for `merged` notifications (dropr:397). A merge is a
//! milestone, but several merges landing close together still read as noise
//! when each posts its own message. This module holds summary-tier `merged`
//! events on the decision cursor for a short window and flushes them as one
//! rolled-up message (`3 pull requests were merged.` + a PR list field).
//!
//! The hold is stateless across restarts on purpose: nothing is buffered in
//! memory. Held entries simply stay un-completed on the cursor, and the
//! flush decision is recomputed each tick from the entries' own `at`
//! timestamps. Error and escalation events are never delayed — a notifying
//! non-merge event queued behind held merges flushes them immediately so
//! delivery order is preserved.

use super::{
    cursor::PendingDecision,
    notifications::{self, Notification},
};
use crate::overseer::{config::DiscordConfig, logging::DecisionEntry};
use chrono::{DateTime, Duration, Utc};
use std::collections::VecDeque;

/// How long the oldest held merge may wait for more merges before the
/// rolled-up message is sent. Fixed rather than a config knob: 5 minutes is
/// short enough that a lone merge still reads as timely, and a knob would
/// outlive its usefulness the moment the default is sane.
const WINDOW_MINUTES: i64 = 5;

/// What `notify::next_notification` decided to do with the front of the
/// pending queue.
pub(super) enum Planned {
    /// Consume `count` front entries, sending `notification` when present.
    Consume {
        count: usize,
        notification: Option<Notification>,
    },
    /// Front merged entries are still inside the rollup window with nothing
    /// notifying queued behind them; leave the cursor untouched and replan
    /// on a later tick.
    Hold,
}

/// Plans the front of the pending queue when it starts with a `merged`
/// daemon event that the configured level admits; `None` hands planning
/// back to the caller's normal single-entry path.
pub(super) fn plan_merged(
    config: &DiscordConfig,
    pending: &VecDeque<PendingDecision>,
    now: DateTime<Utc>,
) -> Option<Planned> {
    let first = pending.front()?.entry.as_ref()?;
    if !is_merged_event(first) || notifications::from_decision(config, first).is_none() {
        return None;
    }
    // Walk the prefix of merges plus silent riders (unparseable lines and
    // level-silenced events); the first notifying non-merge entry ends the
    // prefix and forces an immediate flush so it is not delayed.
    let mut count = 0;
    let mut merged: Vec<&DecisionEntry> = Vec::new();
    let mut flush = false;
    for item in pending.iter() {
        match item.entry.as_ref() {
            Some(entry) if is_merged_event(entry) => merged.push(entry),
            Some(entry) if notifications::from_decision(config, entry).is_some() => {
                flush = true;
                break;
            }
            _ => {}
        }
        count += 1;
    }
    let oldest = merged.first().expect("front entry is a merge").at;
    if !flush && now - oldest < Duration::minutes(WINDOW_MINUTES) {
        return Some(Planned::Hold);
    }
    let notification = match merged.as_slice() {
        [single] => notifications::from_decision(config, single),
        several => Some(rolled_up(several)),
    };
    Some(Planned::Consume {
        count,
        notification,
    })
}

fn is_merged_event(entry: &DecisionEntry) -> bool {
    entry.source.as_deref() == Some("daemon_event") && entry.reason == "merged"
}

/// One message for several merges. The PR list lives in a field, not the
/// description: fields are never localized (`localize.rs`), so repo names
/// and links survive translation, and the count-only description caches
/// well per count in the localizer's content-keyed cache.
fn rolled_up(merged: &[&DecisionEntry]) -> Notification {
    let list = merged
        .iter()
        .map(|entry| merge_label(entry))
        .collect::<Vec<_>>()
        .join(", ");
    Notification {
        title: "Merged".into(),
        description: format!("{} pull requests were merged.", merged.len()),
        color: 0x2ecc71,
        fields: vec![notifications::field("PRs", list)],
    }
}

/// `[repo #N](url)` when both are known, degrading to whichever identifying
/// piece the entry carries.
fn merge_label(entry: &DecisionEntry) -> String {
    let repo = entry.repo.as_deref();
    match entry.pr_url.as_deref() {
        Some(url) => {
            let number = url.rsplit_once("/pull/").map(|(_, number)| number);
            let text = match (repo, number) {
                (Some(repo), Some(number)) => format!("{repo} #{number}"),
                (Some(repo), None) => repo.to_string(),
                (None, Some(number)) => format!("#{number}"),
                (None, None) => url.to_string(),
            };
            format!("[{text}]({url})")
        }
        None => repo
            .map(str::to_owned)
            .or_else(|| entry.task.clone())
            .unwrap_or_else(|| "unknown".into()),
    }
}

#[cfg(test)]
#[path = "rollup_tests.rs"]
mod tests;
