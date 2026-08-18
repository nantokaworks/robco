//! The reviewer's answer, and the boundary of what it is allowed to say.
//!
//! The schema carries a severity and a sentence. It carries no action, no task
//! to dispatch, no pull request to merge, and `deny_unknown_fields` rejects any
//! attempt to add one — so a reviewer that decides it should unblock something
//! produces a parse error, not an unblock.

use serde::Deserialize;

/// Most findings accepted from one review. A model that answers with fifty
/// items is not diagnosing, and each accepted finding becomes a decision entry.
const MAX_FINDINGS: usize = 5;
/// Longest accepted finding sentence.
const MAX_SUMMARY_CHARS: usize = 240;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(super) enum Severity {
    Info,
    Warn,
    Critical,
}

impl Severity {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Critical => "critical",
        }
    }

    /// Whether a finding of this severity is worth an operator's attention now.
    /// An `info` note is recorded but does not raise an escalation, so a chatty
    /// reviewer cannot page anyone.
    pub(super) fn escalates(self) -> bool {
        matches!(self, Self::Warn | Self::Critical)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Review {
    pub summary: String,
    pub findings: Vec<Finding>,
    pub rows: Vec<RowAnswer>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Finding {
    pub severity: Severity,
    pub summary: String,
}

/// One row's one-sentence description, keyed by the `id` the row was offered
/// under (`rows::RowCase::id` — an Inbox item's `target_id`). Carries no
/// severity and nothing else: this is the one place in the schema whose only
/// purpose is prose about a case, never a verdict on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RowAnswer {
    pub id: String,
    pub sentence: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawReview {
    summary: String,
    findings: Vec<RawFinding>,
    #[serde(default)]
    rows: Vec<RawRowAnswer>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFinding {
    severity: Severity,
    summary: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRowAnswer {
    id: String,
    sentence: String,
}

pub(super) fn is_complete(raw: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(raw).is_ok()
}

pub(super) fn parse(raw: &[u8]) -> Result<Review, String> {
    let value: RawReview = serde_json::from_slice(raw).map_err(|error| error.to_string())?;
    if value.summary.trim().is_empty() {
        return Err("summary must not be blank".into());
    }
    let mut findings = Vec::new();
    for finding in value.findings.into_iter().take(MAX_FINDINGS) {
        if finding.summary.trim().is_empty() {
            return Err("finding summary must not be blank".into());
        }
        findings.push(Finding {
            severity: finding.severity,
            summary: clamp(&finding.summary),
        });
    }
    let mut rows = Vec::new();
    for row in value.rows.into_iter().take(super::rows::MAX_ROWS) {
        let id = row.id.trim();
        let sentence = collapse(row.sentence.trim());
        // A row answer is purely additive on top of `summary` and
        // `findings`, so a malformed or overlong one is dropped rather than
        // failing the whole review — the same "fall back, never blank" rule
        // an unmatched `id` gets at record time (dropr:462).
        if id.is_empty() || sentence.is_empty() || sentence.chars().count() > MAX_SUMMARY_CHARS {
            continue;
        }
        rows.push(RowAnswer {
            id: id.to_string(),
            sentence,
        });
    }
    Ok(Review {
        summary: clamp(&value.summary),
        findings,
        rows,
    })
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clamp(text: &str) -> String {
    let collapsed = collapse(text);
    if collapsed.chars().count() <= MAX_SUMMARY_CHARS {
        return collapsed;
    }
    collapsed
        .chars()
        .take(MAX_SUMMARY_CHARS)
        .chain("…".chars())
        .collect()
}
