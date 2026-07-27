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

use crate::overseer::ledger::{LedgerEntry, LedgerPhase};

/// Marks `entry` as escalated by the hold cap, so it is reconsidered instead of
/// being left for good the way other escalations are.
pub(super) fn escalated(entry: &mut LedgerEntry) {
    entry.merge_hold_cap_escalated = true;
    entry.merge_hold_rechecks = 0;
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
/// Charges the attempt regardless of what this pass finds, so a condition
/// that never clears still stops being reconsidered instead of polling
/// forever; an escalation from a judge veto or a closed pull request never
/// sets either signal, so this budget leaves those alone.
pub(super) fn due(entry: &mut LedgerEntry, max: u32) -> bool {
    if entry.phase != LedgerPhase::Escalated {
        return false;
    }
    if !entry.merge_hold_cap_escalated && !entry.merge_hold.escalated {
        return false;
    }
    if entry.merge_hold_rechecks >= max {
        return false;
    }
    entry.merge_hold_cap_escalated = true;
    entry.merge_hold_rechecks = entry.merge_hold_rechecks.saturating_add(1);
    true
}

/// Retires the marker once the entry leaves `Escalated` for good by merging.
pub(super) fn settle(entry: &mut LedgerEntry) {
    entry.merge_hold_cap_escalated = false;
    entry.merge_hold_rechecks = 0;
}

#[cfg(test)]
#[path = "merge_hold_recheck_tests.rs"]
mod tests;
