//! Reading a pull request's own facts off the same `gh pr view` payload the
//! gate already read — the title, size, and failing check an Inbox row shows
//! beside its static guidance (dropr:461).
//!
//! `additions` / `deletions` / `changedFiles` are read here purely for
//! display — this module carries no decision logic of its own, only the
//! facts an Inbox row shows.

use serde_json::Value;

use super::check_rollup;
use crate::overseer::ledger::PrFacts;

/// `None` when GitHub did not answer the size fields at all — a read that
/// answered nothing is not a pull request with a size of zero, and showing
/// one would misreport it. A missing title still renders as an empty one:
/// the size is the fact worth gating on, not the label.
pub(super) fn extract(value: &Value) -> Option<PrFacts> {
    let additions = value.get("additions").and_then(Value::as_u64)?;
    let deletions = value.get("deletions").and_then(Value::as_u64)?;
    let files_changed = value.get("changedFiles").and_then(Value::as_u64)?;
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let failed_checks = value
        .get("statusCheckRollup")
        .and_then(Value::as_array)
        .map(|rollup| check_rollup::failed_names(rollup))
        .unwrap_or_default();
    Some(PrFacts {
        title,
        files_changed: files_changed.min(u64::from(u32::MAX)) as u32,
        lines_changed: additions.saturating_add(deletions).min(u64::from(u32::MAX)) as u32,
        failed_checks,
    })
}

#[cfg(test)]
#[path = "pr_facts_tests.rs"]
mod tests;
