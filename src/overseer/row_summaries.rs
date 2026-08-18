//! One-sentence, model-written descriptions of Inbox rows.
//!
//! The board review (`overseer::review`) is the only writer; the TUI's Inbox
//! aggregation and Discord's `!inbox` are readers, through
//! `ui::inbox::InboxItem::sentence`. Kept in its own file rather than on the
//! ledger, because not every Inbox row is backed by a ledger entry — a
//! release-pipeline skip or a global alert has no pull request to attach a
//! fact to, but can still get a one-line summary.
//!
//! A row's identity (`target_id`) is not enough on its own: the same target
//! can escalate again for a different reason, or the same reason can carry
//! different facts on a later revision. `signature` pins a stored sentence to
//! the exact case it was written about — see
//! `ui::inbox::InboxItem::case_signature` — so a summary about a case that has
//! since changed is never read back as current (dropr:462).

use std::{collections::BTreeMap, fs, path::Path};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::statefile::atomic_replace;
use crate::Result;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RowSummary {
    pub sentence: String,
    pub signature: String,
    pub generated_at: DateTime<Utc>,
}

/// The persisted table, keyed by `target_id`. Rewritten only by the board
/// review, each time it hears back from a model.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RowSummaries {
    #[serde(default)]
    entries: BTreeMap<String, RowSummary>,
}

impl RowSummaries {
    pub fn load() -> Result<Self> {
        Self::load_from(&super::row_summaries_path()?)
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        match fs::read(path) {
            // A corrupt table must not take the Inbox down with it: the worst
            // outcome of ignoring it is that rows show no summary, which the
            // next successful review pass rewrites anyway.
            Ok(raw) => Ok(serde_json::from_slice(&raw).unwrap_or_else(|error| {
                eprintln!("warning: ignoring unreadable row summary table: {error}");
                Self::default()
            })),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        atomic_replace(path, &serde_json::to_vec_pretty(self)?)
    }

    /// The sentence written for `target_id`, if one exists and its signature
    /// still matches the case as of this call — the row's current reason and
    /// facts, computed the same way when the summary was written and now.
    pub fn get(&self, target_id: &str, signature: &str) -> Option<&str> {
        self.entries
            .get(target_id)
            .filter(|summary| summary.signature == signature)
            .map(|summary| summary.sentence.as_str())
    }

    pub fn upsert(&mut self, target_id: String, summary: RowSummary) {
        self.entries.insert(target_id, summary);
    }

    /// Drops every entry `is_live` does not vouch for — a target the board
    /// review no longer sees in the Inbox at all, not merely one it did not
    /// get an answer for this pass. Without this, a target that scrolled off
    /// the Inbox for good would keep its summary in the table forever.
    pub fn retain_live(&mut self, is_live: impl Fn(&str) -> bool) {
        self.entries.retain(|target_id, _| is_live(target_id));
    }
}

#[cfg(test)]
#[path = "row_summaries_tests.rs"]
mod tests;
