//! Telling a merge-gate escalation nothing will reconsider from one the
//! recheck/recovery machinery may still resolve on its own, so the
//! notification layer can stop paging an operator on every pass through a
//! self-healing conflict (dropr:374).
//!
//! Two buckets, named once here rather than string-matched at each call
//! site:
//!
//! - **Terminal** — nothing will reconsider this: the recheck budget
//!   (`merge_hold_recheck::exhausted`) or the recovery budget
//!   (`merge_recovery::CAP_REACHED`) is spent, the worker's session is gone
//!   (`merge_recovery`'s `missing_session` skip), or the pull request closed
//!   without merging — reopening one is a human act. These notify
//!   immediately.
//! - **Transient** — `merge_hold::cap_reached`: the hold budget alone is
//!   spent, but `merge_hold_recheck::escalated` already granted the entry
//!   free reconsideration passes. Something may still look again, so this
//!   notifies only once it has sat unresolved past [`STUCK_AFTER`].
//!
//! Every other escalation reason (a branch-update cap, an unprotected base) is
//! outside this vocabulary — none of them name a
//! condition this module's own recheck/recovery budgets track. Left
//! completely unmanaged, an entry the merge-hold recheck loop keeps
//! reconsidering for free (see `merge_hold_recheck`'s module doc) can hit one
//! of these on every pass with an unchanged reason and head, and each hit
//! logged a fresh `Escalate` decision with no `escalation_notify` at all —
//! which the notification layer's catch-all then read as a brand new alert,
//! forever (dropr:414). So this third bucket notifies too, but only once per
//! `(reason, head)` pair: [`notify`] compares against
//! `LedgerEntry::escalation_notified_reason` /
//! `escalation_notified_head`, the pair the last notification recorded, and
//! suppresses a repeat of the identical pair. A changed reason, a changed
//! head, or the entry leaving the escalation behind (`merge_hold_recheck::settle`,
//! a `merge_recovery` handback) all clear or replace that pair, so the first
//! occurrence of a genuinely new condition still notifies immediately.
//!
//! [`sweep_stuck`] is the other half: a transient escalation that never
//! changes is reconsidered for free, forever (see `merge_hold_recheck`'s
//! module doc) — so it never reaches its own recheck-exhausted notification.
//! Left alone that is silence with no bound, exactly the case an operator
//! most needs to hear about. The sweep watches every entry still eligible
//! for reconsideration and speaks once its age crosses the threshold.

use chrono::{DateTime, Duration, Utc};

use super::{merge_hold_recheck, merge_recovery};
use crate::{
    Result,
    overseer::{
        ledger::{Ledger, LedgerEntry},
        logging::{self, DecisionEntry, DecisionKind},
    },
};

/// The `DecisionEntry::escalation_notify` value an `Escalate` decision with
/// `reason` and revision `head` should carry, for the entry the decision is
/// about. The single call point every merge-subsystem decision builder
/// (`merge_decision`, `merge_recovery`) uses, so the vocabulary is matched
/// once rather than re-derived at each call site.
///
/// Mutates `entry`'s `escalation_notified_reason` / `escalation_notified_head`
/// for the third bucket below — a read-only signature could not implement the
/// "notify once per pair" rule, since deciding *and* remembering has to be one
/// atomic step or two concurrent calls (there are none today; the merge pass
/// is sequential per repository) could both read "unseen" and both notify.
pub(super) fn notify(
    entry: &mut LedgerEntry,
    kind: DecisionKind,
    reason: &str,
    head: &str,
) -> Option<bool> {
    if kind != DecisionKind::Escalate {
        return None;
    }
    if is_terminal(reason) {
        return Some(true);
    }
    if is_transient(reason) {
        return Some(false);
    }
    let unchanged = entry.escalation_notified_reason.as_deref() == Some(reason)
        && entry.escalation_notified_head.as_deref() == Some(head);
    entry.escalation_notified_reason = Some(reason.to_owned());
    entry.escalation_notified_head = Some(head.to_owned());
    Some(!unchanged)
}

/// How long a still-reconsiderable escalation may sit unresolved before
/// [`sweep_stuck`] notifies about it anyway.
///
/// Long enough that an ordinary handback-and-retry cycle — hold, hand back,
/// worker pushes a fix, hold clears — never crosses it: that path resolves
/// in minutes, not hours. Short enough that a genuinely stuck pull request
/// still reaches an operator the same day rather than sitting silent for as
/// long as the daemon runs, which is the failure this module exists to end.
pub(super) const STUCK_AFTER: Duration = Duration::hours(2);

/// Reasons meaning nothing will reconsider this escalation. Exact matches.
const TERMINAL_EXACT: &[&str] = &[
    merge_recovery::CAP_REACHED,
    super::merge_decision::CLOSED_UNMERGED,
];

/// Reasons meaning nothing will reconsider this escalation. Prefix matches,
/// because each carries the condition it stopped on after the colon.
const TERMINAL_PREFIXES: &[&str] = &[
    "merge_hold_recheck_exhausted:",
    "merge_recovery_skipped:missing_session:",
    // This module's own stuck notification: once logged, the entry has
    // already been spoken for, so a reason built by `stuck` classifies
    // itself as terminal the same way any other now would if re-evaluated.
    STUCK_PREFIX,
];

/// `merge_hold::cap_reached`'s prefix: the hold budget alone is spent, but
/// `merge_hold_recheck` already granted free reconsideration.
const TRANSIENT_PREFIX: &str = "merge_hold_cap_reached:";

const STUCK_PREFIX: &str = "merge_hold_stuck:";

/// Whether `reason` names a merge-gate escalation nothing will reconsider.
/// See the module doc for the vocabulary.
pub(super) fn is_terminal(reason: &str) -> bool {
    TERMINAL_EXACT.contains(&reason)
        || TERMINAL_PREFIXES
            .iter()
            .any(|prefix| reason.starts_with(prefix))
}

/// Whether `reason` names a merge-gate escalation the recheck machinery may
/// still resolve on its own.
pub(super) fn is_transient(reason: &str) -> bool {
    reason.starts_with(TRANSIENT_PREFIX)
}

/// Reason recorded by [`sweep_stuck`] once a transient escalation crosses
/// [`STUCK_AFTER`]. Carries the condition it is still stuck on, the same way
/// `merge_hold_recheck::exhausted` names what a spent budget stopped on.
fn stuck(reason: &str) -> String {
    format!("{STUCK_PREFIX}{reason}")
}

/// Notifies once about every entry still eligible for merge-hold
/// reconsideration that has sat unresolved past [`STUCK_AFTER`].
///
/// Called once per daemon pass, after `merge::auto_merge_pass` — the recheck
/// budget it reads (`merge_hold_recheck::due`) is only meaningful once that
/// pass has run. `entry.settled_at` is lazily stamped with `now` the first
/// time this sweep notices a reconsiderable escalation, since nothing else
/// stamps it for a merge-caused transition (`monitor::settle` only observes
/// transitions `reconcile` itself makes, one pass before the merge gate
/// runs). That lazy stamp is accurate to within one poll interval, which is
/// well inside the margin [`STUCK_AFTER`] is chosen with.
pub(super) fn sweep_stuck(
    ledger: &mut Ledger,
    now: DateTime<Utc>,
    max_rechecks: u32,
) -> Result<()> {
    for entry in &mut ledger.entries {
        if entry.merge_hold_stuck_notified {
            continue;
        }
        if !merge_hold_recheck::due(entry, max_rechecks) {
            continue;
        }
        let since = *entry.settled_at.get_or_insert(now);
        if now.signed_duration_since(since) < STUCK_AFTER {
            continue;
        }
        let reason = entry
            .merge_hold
            .reason
            .clone()
            .unwrap_or_else(|| "unknown".into());
        entry.merge_hold_stuck_notified = true;
        log(entry, &stuck(&reason))?;
    }
    Ok(())
}

fn log(entry: &LedgerEntry, reason: &str) -> Result<()> {
    let mut decision = DecisionEntry::new(DecisionKind::Escalate, reason);
    decision.task = Some(entry.task_id.clone());
    decision.repo = Some(entry.repo.clone());
    decision.pr_url = entry.pr_url.clone();
    decision.source = Some("merge_escalation".into());
    decision.escalation_notify = Some(true);
    logging::append(&decision)
}

#[cfg(test)]
#[path = "merge_escalation_tests.rs"]
mod tests;
