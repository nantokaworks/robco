//! Whether the autonomy envelope allows one pull request the deterministic
//! gate cleared to merge.
//!
//! Split out to keep the caller under its size limit: the envelope check and
//! the operator-bypass handling are one cohesive unit that
//! `merge_evaluate::evaluate` only needs to invoke, not inline.

use serde_json::Value;

use super::merge_decision::{Halt, log};
use crate::{
    Result,
    config::Config,
    overseer::{
        autonomy::{ChangeFacts, Decision, merge_envelope_decision},
        ledger::LedgerEntry,
        logging::DecisionKind,
    },
};

/// What the autonomy envelope said about a pull request the deterministic
/// gate cleared.
pub(super) enum Judgment {
    Allow,
    /// The autonomy envelope stopped the merge under this reason.
    Halt(Halt),
}

pub(super) fn change_facts(value: &Value, consecutive_failures: u32) -> ChangeFacts {
    let files = value.get("files").and_then(Value::as_array);
    let additions = value.get("additions").and_then(Value::as_u64);
    let deletions = value.get("deletions").and_then(Value::as_u64);
    let changed = value.get("changedFiles").and_then(Value::as_u64);
    let paths = files
        .into_iter()
        .flatten()
        .filter_map(|file| file.get("path").and_then(Value::as_str))
        .collect::<Vec<_>>();
    let files_known = files.is_some_and(|items| {
        items
            .iter()
            .all(|file| file.get("path").and_then(Value::as_str).is_some())
            && changed.is_some_and(|count| count == paths.len() as u64)
    });
    let only_docs_or_tests = !paths.is_empty()
        && paths.iter().all(|path| {
            path.starts_with("docs/")
                || path.starts_with("tests/")
                || path.contains("/tests/")
                || path.ends_with(".md")
                || path.ends_with("_test.rs")
                || path.ends_with("_tests.rs")
        });
    let contains = |needles: &[&str]| {
        paths
            .iter()
            .any(|path| needles.iter().any(|needle| path.contains(needle)))
    };
    ChangeFacts {
        facts_known: additions.is_some() && deletions.is_some() && changed.is_some() && files_known,
        files_changed: changed.unwrap_or(0).min(u32::MAX as u64) as u32,
        lines_changed: additions
            .unwrap_or(0)
            .saturating_add(deletions.unwrap_or(0))
            .min(u32::MAX as u64) as u32,
        only_docs_or_tests,
        touches_security: contains(&["security", "auth", "permission", "secret"]),
        touches_dependencies: contains(&["Cargo.toml", "Cargo.lock", "package.json", "lockfile"]),
        touches_prod_or_ci: contains(&[".github/", "Dockerfile", "deploy", "production"]),
        consecutive_failures,
        ..ChangeFacts::default()
    }
}

/// Whether the autonomy envelope allows `entry`'s pull request to merge, now
/// that the deterministic gate has cleared it.
pub(super) fn merge_allows(
    entry: &mut LedgerEntry,
    value: &Value,
    config: &Config,
    consecutive_failures: u32,
) -> Result<Judgment> {
    let facts = change_facts(value, consecutive_failures);
    let head = crate::overseer::daemon::pull_request::head_sha(value);
    if !matches!(
        merge_envelope_decision(true, true, &facts, &config.overseer),
        Decision::Escalate(_)
    ) {
        return Ok(Judgment::Allow);
    }
    if take_operator_override(entry, head, "autonomy_envelope")? || take_merge_approval(entry, head)? {
        return Ok(Judgment::Allow);
    }
    Ok(Judgment::Halt(Halt::escalate("autonomy_envelope")))
}

/// Consumes `entry.operator_override` if it is still live and its head
/// matches the pull request's current one, logging the bypass under what it
/// bypassed.
///
/// Matching on the exact head is what keeps the bypass scoped to the
/// revision the operator actually approved (see `ledger::OperatorOverride`):
/// a later push presents a head the operator never saw, and that revision
/// must clear the gate — or earn its own override — on its own. Taken
/// (cleared) either way, matched or not: an override granted for a head this
/// pull request has since moved past is spent, not saved for a revision it
/// was never granted for.
pub(super) fn take_operator_override(
    entry: &mut LedgerEntry,
    head: &str,
    bypassed: &str,
) -> Result<bool> {
    let Some(granted) = entry.operator_override.take() else {
        return Ok(false);
    };
    if granted.head != head {
        return Ok(false);
    }
    log(
        entry,
        DecisionKind::Merge,
        &format!("operator_override:{bypassed}"),
        head,
    )?;
    Ok(true)
}

/// Consumes `entry.merge_approval` if it is still live and its head matches
/// the pull request's current one — the approval Discord's `!merge` queued
/// (see `discord::ledger_requests::LedgerRequest::Approve`). This is the
/// bypass an entry's own reconsideration reaches this arm to spend: the
/// approval also reset `merge_hold_recheck`
/// (`discord::ledger_requests::record_approval`), which is what let this
/// entry's escalated phase be looked at again in the first place.
///
/// Only the autonomy envelope's decision to escalate is ever bypassed here.
/// Taken (cleared) either way, matched or not: a head that no longer matches
/// means the worker pushed after the operator approved, and the drop is
/// recorded so it does not look, later, like a merge that should already
/// have happened.
pub(super) fn take_merge_approval(entry: &mut LedgerEntry, head: &str) -> Result<bool> {
    let Some(granted) = entry.merge_approval.take() else {
        return Ok(false);
    };
    if granted.head != head {
        log(
            entry,
            DecisionKind::Hold,
            "merge_approval_dropped:stale_head",
            head,
        )?;
        return Ok(false);
    }
    log(
        entry,
        DecisionKind::Merge,
        "merge_approval:autonomy_envelope",
        head,
    )?;
    Ok(true)
}

#[cfg(test)]
#[path = "merge_allow_tests.rs"]
mod tests;
