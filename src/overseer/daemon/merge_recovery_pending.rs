//! Remembering a merge-recovery handback the worker's session was too busy
//! to receive (dropr:530).
//!
//! `merge_delivery::is_busy` tells `merge_recovery::dispatch` the worker's
//! session is mid-turn before it ever types a prompt into it. Refusing that
//! send is correct — typing into a live turn corrupts whatever the worker is
//! composing, the same reason `mcp::tools::report::guard_delivery` refuses a
//! `robco report` into a busy session. What must not happen next is
//! silence: the daemon already runs `dispatch` again on its ordinary poll
//! cadence, so a withheld handback only needs to be remembered until a later
//! pass finds the worker idle. This module is that memory.
//!
//! The memory lives on `LedgerEntry.merge_recovery.pending`
//! ([`PendingHandback`]), not in the daemon process, so a restart does not
//! forget a worker is still owed a handback.

use crate::overseer::ledger::{LedgerEntry, PendingHandback};

/// What `withhold` decided about one busy-session attempt.
pub(super) enum Outcome {
    /// Held for a later pass to retry, carrying the reason recorded in the
    /// decision log.
    Held(String),
    /// The retry bound is spent — the worker was never told, and a session
    /// that is never idle is not going to become one. Carries the reason and
    /// head the escalation is recorded against.
    Abandoned { reason: String, head: String },
}

/// Withholds this pass's handback because `reason`'s worker session is
/// mid-turn, refunding the charge `plan` took on the way in — the same
/// leave-the-budget-untouched shape `super::refund` gives a confirm that
/// never arrives, since nothing was actually sent here either. Bounded by
/// `max_recoveries` through the entry's own attempt count, independent of
/// `charged`: `refund` clears the latter every pass this runs, so reusing it
/// here would let a session that is never idle retry forever.
pub(super) fn withhold(entry: &mut LedgerEntry, reason: &str, max_recoveries: u32) -> Outcome {
    let head = entry.merge_recovery.head.clone().unwrap_or_default();
    let base = entry.merge_recovery.base.clone().unwrap_or_default();
    super::refund(entry);
    let attempts = hold(entry, reason, &head, &base);
    if attempts >= max_recoveries {
        discard(entry);
        return Outcome::Abandoned {
            reason: pending_abandoned(reason),
            head,
        };
    }
    Outcome::Held(withheld(reason))
}

/// Reason recorded when a handback is withheld because the worker's session
/// is mid-turn. Distinct from `merge_recovery::skipped`: this one is
/// retried on a later pass, not a dead end recorded once and left alone.
fn withheld(reason: &str) -> String {
    format!("merge_recovery_pending:{reason}")
}

/// Reason recorded when a withheld handback exhausts its own retry bound.
/// Named apart from `merge_recovery::undeliverable` (a send that could not
/// be confirmed) because the two describe different failures: this one
/// never got as far as typing anything into the session at all.
fn pending_abandoned(reason: &str) -> String {
    format!("merge_recovery_pending_abandoned:{reason}")
}

/// Records that `reason` (for `head`/`base`) could not be delivered because
/// the worker's session was busy, replacing whatever handback this entry was
/// already holding.
///
/// `hold` always overwrites: the worker needs the current failure, not a
/// queue of every one that came before it. The returned attempt count is
/// only carried forward from the previous record when `reason`, `head`, and
/// `base` all match it — a changed one is a genuinely different instruction,
/// so it starts its own count rather than inheriting a budget spent on the
/// one it replaces.
fn hold(entry: &mut LedgerEntry, reason: &str, head: &str, base: &str) -> u32 {
    let attempts = match &entry.merge_recovery.pending {
        Some(pending) if same_instruction(pending, reason, head, base) => {
            pending.attempts.saturating_add(1)
        }
        _ => 1,
    };
    entry.merge_recovery.pending = Some(PendingHandback {
        reason: reason.to_owned(),
        head: head.to_owned(),
        base: base.to_owned(),
        attempts,
    });
    attempts
}

fn same_instruction(pending: &PendingHandback, reason: &str, head: &str, base: &str) -> bool {
    pending.reason == reason && pending.head == head && pending.base == base
}

/// Forgets a pending handback this entry no longer needs: delivered,
/// abandoned past its retry bound, or moot because the entry left the state
/// the handback was recorded for — the pull request merged, or the entry
/// escalated through a path that has nothing to do with merge recovery.
///
/// Discarding rather than leaving the stale record behind matters beyond
/// tidiness: a pending handback still on the entry is what
/// `ui::overseer::hold_reason` shows the operator as "waiting for the
/// worker to be idle", and a delivered or abandoned one that no longer holds
/// must not keep saying that.
pub fn discard(entry: &mut LedgerEntry) {
    entry.merge_recovery.pending = None;
}

#[cfg(test)]
#[path = "merge_recovery_pending_tests.rs"]
mod tests;
