//! Inbox items the operator has cleared.
//!
//! The Overseer Inbox is not a mailbox: it is re-derived on every refresh from
//! `decisions.jsonl`, `ledger.json`, and the registry (see
//! [`crate::ui::inbox::aggregate`]). Dismissing a row therefore cannot delete a
//! record — it records a suppression that the aggregation applies as a filter.
//!
//! A suppression is keyed on the item's `(kind, target_id)` identity, the same
//! pair the aggregation dedupes on, plus the timestamp the item carried when it
//! was dismissed. Only items at or before that timestamp are hidden: a *newer*
//! escalation for the same target has a newer `at` and comes back. That is the
//! load-bearing half of the design — without it, clearing one stale alert would
//! silently mute the task it came from forever.

use std::{collections::HashSet, fs, path::Path};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::statefile::{atomic_replace, with_lock};
use crate::Result;

/// One suppressed identity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Dismissal {
    /// `InboxKind::code` of the dismissed item — the kind half of its identity.
    pub kind: String,
    pub target_id: String,
    /// The `at` of the item at the moment it was dismissed. Anything at or
    /// before this is the alert the operator cleared; anything after it is new.
    pub dismissed_through: DateTime<Utc>,
}

/// The persisted suppression list. Read on every Inbox aggregation, rewritten
/// only when the operator dismisses something.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Dismissals {
    #[serde(default)]
    pub entries: Vec<Dismissal>,
}

impl Dismissals {
    pub fn load() -> Result<Self> {
        Self::load_from(&super::inbox_dismissals_path()?)
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        match fs::read(path) {
            // A corrupt list must not take the Inbox down with it: the worst
            // outcome of ignoring it is that dismissed rows come back, which the
            // operator can see and redo.
            Ok(raw) => Ok(serde_json::from_slice(&raw).unwrap_or_else(|error| {
                eprintln!("warning: ignoring unreadable inbox dismissal list: {error}");
                Self::default()
            })),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        atomic_replace(path, &serde_json::to_vec_pretty(self)?)
    }

    /// True when this item is an alert the operator already cleared.
    pub fn suppresses(&self, kind: &str, target_id: &str, at: DateTime<Utc>) -> bool {
        self.entries.iter().any(|entry| {
            entry.kind == kind && entry.target_id == target_id && at <= entry.dismissed_through
        })
    }

    /// Record a dismissal, or carry an existing one forward to a newer
    /// timestamp. Never moves `dismissed_through` backwards — re-dismissing an
    /// item the sources have since re-raised must not un-hide older alerts.
    pub fn dismiss(&mut self, kind: &str, target_id: &str, at: DateTime<Utc>) {
        match self
            .entries
            .iter_mut()
            .find(|entry| entry.kind == kind && entry.target_id == target_id)
        {
            Some(entry) => entry.dismissed_through = entry.dismissed_through.max(at),
            None => self.entries.push(Dismissal {
                kind: kind.to_string(),
                target_id: target_id.to_string(),
                dismissed_through: at,
            }),
        }
    }

    /// Drop entries for targets no source produces any more, so the file does
    /// not grow one row per escalation the Overseer has ever raised.
    ///
    /// `is_live` must be asked about the *unfiltered* aggregation. Pruning
    /// against what the Inbox currently displays would delete the very entries
    /// doing the hiding, and every dismissed row would return on the next
    /// refresh.
    pub fn retain_live(&mut self, is_live: impl Fn(&str, &str) -> bool) {
        self.entries
            .retain(|entry| is_live(&entry.kind, &entry.target_id));
    }
}

/// A single item to suppress: its identity and the timestamp it carried.
pub type DismissTarget<'a> = (&'a str, &'a str, DateTime<Utc>);

/// Suppress `items` and prune the list against the live identities in one
/// locked read-modify-write, so a concurrent dismissal cannot lose the other's
/// entry.
pub fn dismiss(items: &[DismissTarget<'_>], live: &HashSet<(String, String)>) -> Result<()> {
    dismiss_at(&super::inbox_dismissals_path()?, items, live)
}

pub fn dismiss_at(
    path: &Path,
    items: &[DismissTarget<'_>],
    live: &HashSet<(String, String)>,
) -> Result<()> {
    with_lock(path, || {
        let mut dismissals = Dismissals::load_from(path)?;
        // Prune before recording, not after: the rows being dismissed are by
        // definition live, so pruning last would depend on the caller's `live`
        // set agreeing — and an empty or stale one would silently discard the
        // dismissal the operator just asked for.
        dismissals.retain_live(|kind, target_id| {
            live.contains(&(kind.to_string(), target_id.to_string()))
        });
        for (kind, target_id, at) in items {
            dismissals.dismiss(kind, target_id, *at);
        }
        dismissals.save_to(path)
    })
}

#[cfg(test)]
#[path = "dismissals_tests.rs"]
mod tests;
