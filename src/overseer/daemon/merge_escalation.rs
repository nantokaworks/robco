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
//!   (`merge_recovery`'s `missing_session` skip), the judge fail-safe cap
//!   tripped, or the pull request closed without merging — reopening one is
//!   a human act. These notify immediately.
//! - **Transient** — `merge_hold::cap_reached`: the hold budget alone is
//!   spent, but `merge_hold_recheck::escalated` already granted the entry
//!   free reconsideration passes. Something may still look again, so this
//!   notifies only once it has sat unresolved past [`STUCK_AFTER`].
//!
//! Every other escalation reason (a judge veto, the autonomy envelope, a
//! branch-update cap) is outside this vocabulary on purpose — the task this
//! module exists for scopes the new policy to the merge-hold recheck loop,
//! not to every escalation path the gate can take. Those keep notifying
//! exactly as they did before this module existed.
//!
//! [`sweep_stuck`] is the other half: a transient escalation that never
//! changes is reconsidered for free, forever (see `merge_hold_recheck`'s
//! module doc) — so it never reaches its own recheck-exhausted notification.
//! Left alone that is silence with no bound, exactly the case an operator
//! most needs to hear about. The sweep watches every entry still eligible
//! for reconsideration and speaks once its age crosses the threshold.

use chrono::{DateTime, Duration, Utc};

use super::{merge_hold_recheck, merge_judge_fail_safe, merge_recovery};
use crate::{
    Result,
    overseer::{
        ledger::{Ledger, LedgerEntry},
        logging::{self, DecisionEntry, DecisionKind},
    },
};

/// The `DecisionEntry::escalation_notify` value an `Escalate` decision with
/// `reason` should carry. `None` for every reason outside this module's
/// terminal/transient vocabulary — including escalation reasons this task's
/// scope leaves untouched (a judge veto, the autonomy envelope, a
/// branch-update cap) — so the notification layer falls back to its
/// pre-existing catch-all for them, unchanged. The single call point every
/// merge-subsystem decision builder (`merge_decision`, `merge_recovery`,
/// `merge_judge_fail_safe`) uses, so the vocabulary is matched once rather
/// than re-derived at each call site.
pub(super) fn notify(kind: DecisionKind, reason: &str) -> Option<bool> {
    if kind != DecisionKind::Escalate {
        return None;
    }
    if is_terminal(reason) {
        Some(true)
    } else if is_transient(reason) {
        Some(false)
    } else {
        None
    }
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
    merge_judge_fail_safe::CAP_REACHED,
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
