//! The Inbox rows the reviewer describes, one sentence each (dropr:462).
//!
//! A separate concern from [`super::digest::Digest`]: a row comes from
//! `ui::inbox` — ledger and decision-log state resolved against dismissals
//! and live sessions — not the raw decision/ledger arithmetic the digest
//! reduces on its own. Reusing `ui::inbox::current` here, the same function
//! Discord's `!inbox` calls, is what keeps the reviewer's idea of "the
//! Inbox" from drifting from what the TUI and Discord actually show.

use std::{collections::BTreeMap, path::Path};

use chrono::Utc;
use serde::Serialize;

use super::result::RowAnswer;
use crate::{
    Result,
    overseer::row_summaries::{RowSummaries, RowSummary},
    ui::inbox::InboxItem,
};

/// Most rows offered to the reviewer per pass, newest first. Summarizing
/// past this many in one call is diagnosing a backlog, not describing a
/// case — the same reasoning `digest`'s own caps follow.
pub(super) const MAX_ROWS: usize = 20;
/// Longest reason text kept per row, mirroring `digest::MAX_REASON_CHARS`.
const MAX_REASON_CHARS: usize = 160;

/// One row's own case, as the model sees it: its identity, its reason, and
/// its pull request's facts when it has one.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(super) struct RowCase {
    pub id: String,
    pub reason: String,
    pub pr_title: Option<String>,
    pub files_changed: Option<u32>,
    pub lines_changed: Option<u32>,
    pub failed_checks: Vec<String>,
}

/// The current Inbox, reduced to what the reviewer needs to describe each
/// row — capped to [`MAX_ROWS`], newest first, since `items` is already
/// sorted that way by `ui::inbox::aggregate`.
pub(super) fn cases(items: &[InboxItem]) -> Vec<RowCase> {
    items.iter().take(MAX_ROWS).map(from_item).collect()
}

fn from_item(item: &InboxItem) -> RowCase {
    let facts = item.pr_facts.as_ref();
    RowCase {
        id: item.target_id.clone(),
        reason: truncate(&item.detail),
        pr_title: facts
            .map(|facts| facts.title.clone())
            .filter(|title| !title.is_empty()),
        files_changed: facts.map(|facts| facts.files_changed),
        lines_changed: facts.map(|facts| facts.lines_changed),
        failed_checks: facts.map_or_else(Vec::new, |facts| facts.failed_checks.clone()),
    }
}

/// `target_id -> case signature` for every current Inbox row, not only the
/// `MAX_ROWS` offered to the model — [`apply`] prunes stored summaries
/// against this full set, so a row temporarily left out of one pass's
/// prompt does not lose an otherwise still-valid summary from an earlier one.
pub(super) fn pending_signatures(items: &[InboxItem]) -> BTreeMap<String, String> {
    items
        .iter()
        .map(|item| (item.target_id.clone(), item.case_signature()))
        .collect()
}

/// Stores each answer under the signature its row carried in `pending` —
/// captured when the session was spawned, not read again now, so a case that
/// changed while the model was thinking cannot be pinned to the wrong
/// signature (dropr:462) — and drops every stored summary for a target
/// `pending` does not name at all. An answer for an id `pending` does not
/// know about is silently ignored: nothing stops a model from echoing back
/// an id it invented.
pub(super) fn apply(
    path: &Path,
    pending: BTreeMap<String, String>,
    answers: &[RowAnswer],
) -> Result<()> {
    if pending.is_empty() {
        return Ok(());
    }
    let mut summaries = RowSummaries::load_from(path)?;
    summaries.retain_live(|target_id| pending.contains_key(target_id));
    for answer in answers {
        if let Some(signature) = pending.get(&answer.id) {
            summaries.upsert(
                answer.id.clone(),
                RowSummary {
                    sentence: answer.sentence.clone(),
                    signature: signature.clone(),
                    generated_at: Utc::now(),
                },
            );
        }
    }
    summaries.save_to(path)
}

fn truncate(reason: &str) -> String {
    let collapsed = reason.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= MAX_REASON_CHARS {
        return collapsed;
    }
    collapsed
        .chars()
        .take(MAX_REASON_CHARS)
        .chain("…".chars())
        .collect()
}

#[cfg(test)]
#[path = "rows_tests.rs"]
mod tests;
