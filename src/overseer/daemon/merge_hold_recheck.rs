//! Reconsidering an entry the hold cap escalated, once its condition might have
//! cleared.
//!
//! `merge_hold::charge` moves an entry to `Escalated` once its hold budget is
//! spent, but the condition it escalated on — a pre-judge gate like protection,
//! checks, or merge state — is never cached anywhere the ordinary
//! `has_terminal_merge` re-entry check (the judge-veto/escalation path) can
//! see. Left alone, an entry escalated this way never returns to the gate even
//! after an operator fixes exactly the condition it was held on.
//!
//! `merge_hold` itself cannot carry the marker this needs: `merge_hold::cleared`
//! resets it to default the moment a pass gets past whatever it held on —
//! including the very reconsideration pass this module grants — so the signal
//! has to live apart from it, the same way `merge_judge_fail_safe`'s budget
//! lives apart from `merge_hold` for the same reason. But `merge_hold.escalated`
//! is exactly what `merge_hold::charge` already leaves behind on every entry a
//! prior build escalated this way, so `due` also accepts that as proof, which
//! is what lets an already-escalated entry sitting in a ledger written before
//! this module existed start being reconsidered too.
//!
//! What a reconsideration look *costs* follows the same reasoning: a pass that
//! re-reads the gate and finds the identical (reason, head) pair it already
//! saw has not re-checked anything — it has confirmed the condition escalation
//! itself already recorded. Charging that pass anyway is what let a single
//! unresolved conflict burn a ten-look budget in ten minutes, long before an
//! operator could plausibly have acted. So the budget now counts *changes*:
//! [`charge`] compares the reason and head this pass saw against
//! `merge_hold_recheck_reason` / `merge_hold_recheck_head`, the pair the last
//! charged (or escalating) pass recorded, and only spends a look when they
//! differ. A condition that never changes is reconsidered for free,
//! indefinitely — an operator fix reaches the gate on the next pass whenever
//! it lands. A condition that keeps changing (a worker pushing a new failing
//! revision every pass) still spends one look per genuine change and still
//! converges on exhaustion once `max` distinct states have been seen.

use crate::overseer::ledger::{LedgerEntry, LedgerPhase};

/// Marks `entry` as escalated by the hold cap, so it is reconsidered instead of
/// being left for good the way other escalations are.
///
/// `reason` and `head` are the condition the hold cap just reached. Seeding
/// them here means the very first reconsideration pass — which ordinarily
/// finds the same condition still holding, since nothing has had time to
/// change yet — is not treated as a change against an empty baseline and
/// charged for free information escalation itself already recorded.
pub(super) fn escalated(entry: &mut LedgerEntry, reason: &str, head: &str) {
    entry.merge_hold_cap_escalated = true;
    entry.merge_hold_rechecks = 0;
    entry.merge_hold_recheck_reason = Some(reason.to_owned());
    entry.merge_hold_recheck_head = Some(head.to_owned());
}

/// Whether `entry` should be given another look through the gate this pass.
///
/// Qualifies on either `merge_hold_cap_escalated` — set once this module has
/// already reconsidered the entry — or `merge_hold.escalated`, which
/// `merge_hold::charge` sets the moment the cap trips and which never gets
/// cleared for an entry nothing has reconsidered since. The second check is
/// what lets an entry the hold cap escalated *before this module existed* —
/// sitting untouched in a ledger written by an older build — start being
/// reconsidered too, not only ones escalated from here on.
///
/// Reads only. [`charge`] is what spends a look, and the caller charges it on
/// the one outcome the budget is for — a pass that re-read the gate and found
/// it still holding. An escalation from a judge veto or a closed pull request
/// never sets either signal, so this budget leaves those alone.
pub(super) fn due(entry: &LedgerEntry, max: u32) -> bool {
    entry.phase == LedgerPhase::Escalated
        && (entry.merge_hold_cap_escalated || entry.merge_hold.escalated)
        && entry.merge_hold_rechecks < max
}

/// Spends one of the looks [`due`] granted when the pass actually learned
/// something, and reports whether that was the last one.
///
/// Kept apart from `due` because the two questions have different answers on
/// the same pass. A reconsidered entry that clears the gate and waits on a
/// judgment is not being re-checked — the gate already answered — and a
/// judgment round trip runs on the judge queue's own schedule, one session at a
/// time, which on a busy queue outlasts the whole budget. Charging every pass
/// that merely *looked* would therefore spend the budget on waiting rather than
/// on re-checking, and an entry whose verdict arrived one pass too late would be
/// stranded in `Escalated` with nothing left to bring it back: exactly the
/// failure this module exists to end.
///
/// Charging is further narrowed to passes where `reason` or `head` differs
/// from `merge_hold_recheck_reason` / `merge_hold_recheck_head` — the pair the
/// last charged (or escalating) pass recorded. A pass that re-reads the
/// identical pair has not re-checked anything; see the module doc for why that
/// must not spend budget.
///
/// Also promotes `merge_hold.escalated` to this module's own marker, so an
/// entry escalated by an older build is tracked here from its first charge on.
pub(super) fn charge(entry: &mut LedgerEntry, reason: &str, head: &str, max: u32) -> bool {
    entry.merge_hold_cap_escalated = true;
    let unchanged = entry.merge_hold_recheck_reason.as_deref() == Some(reason)
        && entry.merge_hold_recheck_head.as_deref() == Some(head);
    entry.merge_hold_recheck_reason = Some(reason.to_owned());
    entry.merge_hold_recheck_head = Some(head.to_owned());
    if unchanged {
        return false;
    }
    entry.merge_hold_rechecks = entry.merge_hold_rechecks.saturating_add(1);
    entry.merge_hold_rechecks >= max
}

/// Reason recorded on the pass that spends the last look.
///
/// Carries the condition the entry stopped on, because "nothing will reconsider
/// this again" is only actionable together with what it was still held on.
pub(super) fn exhausted(reason: &str) -> String {
    format!("merge_hold_recheck_exhausted:{reason}")
}

/// Retires the marker once the entry leaves `Escalated` for good by merging,
/// or once the deterministic gate clears and a real judge verdict becomes
/// the entry's authority instead (see `merge_judge_gate::judge_allows`).
///
/// Also retires `settled_at` and `merge_hold_stuck_notified` —
/// `merge_escalation::sweep_stuck`'s own markers for the same escalation —
/// so a verdict or a merge that resolves this condition does not leave a
/// stale age behind for whatever the entry does next.
pub(super) fn settle(entry: &mut LedgerEntry) {
    entry.merge_hold_cap_escalated = false;
    entry.merge_hold_rechecks = 0;
    entry.merge_hold_recheck_reason = None;
    entry.merge_hold_recheck_head = None;
    entry.settled_at = None;
    entry.merge_hold_stuck_notified = false;
}

#[cfg(test)]
#[path = "merge_hold_recheck_tests.rs"]
mod tests;
