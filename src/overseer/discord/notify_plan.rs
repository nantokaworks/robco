//! Planning half of decision delivery, split out of `notify.rs` (which keeps
//! the send/localize/retry loop): batch bounding, and turning the front of a
//! pending batch into the notifications one cursor advance stands for.

use super::{
    cursor::PendingDecision,
    notifications::{self, from_decision},
    rollup::{self, Planned},
};
use crate::overseer::{
    config::DiscordConfig,
    ledger::Ledger,
    logging::{DecisionEntry, DecisionKind},
};
use std::collections::{HashMap, VecDeque};

/// A burst needs at least this many distinct (task, reason) alerts to
/// coalesce into a digest embed; smaller bursts go out as individual full
/// notifications, whose Task/Repo/PR fields a digest line can only abbreviate.
const DIGEST_MIN_DISTINCT: usize = 3;

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

/// What the front of `pending` renders as, and how many pending decisions
/// one cursor advance consumes. A digest-shaped run consumes the whole run
/// and renders as one digest embed — or, below `DIGEST_MIN_DISTINCT`, as one
/// full notification per distinct alert. A front `merged` event may plan a
/// rollup or a hold (`rollup::plan_merged`). Anything else consumes one
/// decision and renders as at most one notification.
pub(super) fn next_notification(
    config: &DiscordConfig,
    pending: &VecDeque<PendingDecision>,
    display_ids: &HashMap<String, String>,
    now: chrono::DateTime<chrono::Utc>,
) -> Planned {
    let first = pending.front().expect("non-empty pending decisions");
    if first.entry.as_ref().is_some_and(is_digest_entry) {
        let entries = pending
            .iter()
            .map(|item| item.entry.as_ref())
            .take_while(|entry| entry.is_some_and(is_digest_entry))
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        let count = entries.len();
        let alerts = notifications::dedup_alerts(notifications::digest_alerts(config, &entries));
        if alerts.is_empty() {
            return Planned::Consume {
                count,
                notifications: Vec::new(),
            };
        }
        let planned = if alerts.len() >= DIGEST_MIN_DISTINCT {
            vec![notifications::digest_notification(&alerts, display_ids)]
        } else {
            alerts
                .iter()
                .filter_map(|(entry, _)| from_decision(config, entry))
                .collect()
        };
        return Planned::Consume {
            count,
            notifications: planned,
        };
    }
    if let Some(planned) = rollup::plan_merged(config, pending, now) {
        return planned;
    }
    Planned::Consume {
        count: 1,
        notifications: first
            .entry
            .as_ref()
            .and_then(|entry| from_decision(config, entry))
            .into_iter()
            .collect(),
    }
}

pub(super) fn is_digest_entry(entry: &DecisionEntry) -> bool {
    entry.source.as_deref() != Some("discord")
        && matches!(
            entry.kind,
            DecisionKind::Escalate | DecisionKind::CircuitOpen
        )
}

/// Task nanoid → dropr `#N` display id, read best-effort from the ledger —
/// the only place the daemon already holds that mapping. Loaded only when
/// the batch carries digest-shaped entries; an unreadable ledger just means
/// digest lines fall back to nanoids.
pub(super) fn display_ids(pending: &VecDeque<PendingDecision>) -> HashMap<String, String> {
    if !pending
        .iter()
        .any(|item| item.entry.as_ref().is_some_and(is_digest_entry))
    {
        return HashMap::new();
    }
    let Ok(ledger) = Ledger::load() else {
        return HashMap::new();
    };
    ledger
        .entries
        .into_iter()
        .filter(|entry| !entry.display_id.is_empty())
        .map(|entry| (entry.task_id, entry.display_id))
        .collect()
}

#[cfg(test)]
#[path = "notify_plan_tests.rs"]
mod tests;
