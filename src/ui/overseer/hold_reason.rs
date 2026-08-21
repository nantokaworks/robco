//! Whether a live merge hold says something worth showing on the agent's own
//! tree row (dropr:529).
//!
//! dropr:524 gave a row a second line for a ledger entry that had *stopped* —
//! a terminal phase, `merge_error`. A hold is different: the entry is still
//! `LedgerPhase::PrOpened`, alive, and may still clear on its own. So this
//! reads `entry.merge_hold.reason` directly, the same field
//! `ui::overseer::decisions::merge_hold_detail` already reads for the Info
//! pane — live and self-clearing on the very next auto-merge pass, so no
//! decision-log tie-breaker is needed the way
//! `ui::overseer::decisions::terminal_reason` needs one.
//!
//! Kept out of `decisions.rs`: that file is already at the source-size limit
//! (see its own split-out test files), and this needs no helper it defines.
//!
//! Not every hold earns a line. `merge_hold::charge`
//! (`overseer::daemon::merge_hold`) already keeps `behind_*` and
//! `prerequisite_unmerged:*` reasons out of this field entirely — waiting for
//! the merge queue's turn, or for a different task's pull request, is already
//! the expected steady state, so those never reach here to begin with. Of the
//! reasons that do reach the field, only `checks_waiting` is quiet by
//! default: checks that just started are the system working, not a
//! condition anyone needs to act on. Every other reason — `checks_not_green`
//! foremost among them — means nothing will change without the worker or an
//! operator doing something, so it earns a line the moment it is recorded.
//! See the dropr:529 decision scribble for the full reasoning.
//!
//! dropr:534 adds a second, unrelated source: `entry.approval_dropped`, set
//! by `daemon::merge_allow::take_merge_approval` when a worker's push moved
//! the pull request past the head an operator's `m` approved and the push
//! did not qualify to carry the approval forward. That event is a
//! `DecisionKind::Hold` written straight to `decisions.jsonl` by
//! `merge_allow` itself — it never touches `entry.merge_hold`, so
//! `merge_hold::charge`'s own budget-scoped field never sees it and this
//! module has to read it separately. Checked first and unconditionally
//! (no quiet window): a dropped approval means the key the operator pressed
//! stopped counting, and burying that behind `checks_waiting`'s noise
//! filter would recreate the exact silence dropr:534 exists to end.
//!
//! dropr:530 adds a third source: `entry.merge_recovery.pending`, set by
//! `daemon::merge_recovery_pending::hold` when the worker's session was too
//! busy to receive a handback. This complements, rather than substitutes
//! for, `daemon::merge_recovery`'s own handback into the worker's session —
//! the row tells the operator, the handback tells the worker. Checked ahead
//! of `merge_hold.reason` and its quiet window for the same reason
//! `approval_dropped` is: a queued-but-undelivered instruction is new,
//! specific information the underlying hold reason alone does not carry.

use crate::overseer::discord::humanize;
use crate::overseer::ledger::{Ledger, LedgerPhase};

/// Reason recorded while a pull request's checks are still running. Quiet by
/// itself — see [`QUIET_HOLD_PASSES`].
const CHECKS_WAITING: &str = "checks_waiting";

/// Passes `checks_waiting` must survive before it earns a line.
///
/// `merge_hold::charge` counts consecutive passes held on the same
/// (reason, head) pair, reaching 1 on the first pass a hold is observed. A
/// threshold of 3 means the reason must still be `checks_waiting` on the
/// third consecutive poll — roughly two poll intervals after it was first
/// seen, about two minutes at the default 60s `poll_interval_secs` — before
/// the row says anything. That clears the "PR opened ten seconds ago" noise
/// case while staying far under `max_merge_holds` (default 30 passes), so
/// the line is never a last-second surprise right before the hold escalates.
const QUIET_HOLD_PASSES: u32 = 3;

/// The reason `agent_id`'s pull request is being held, worth a line on its
/// own row, or `None` when there is nothing to say.
///
/// `None` covers: no ledger entry, a phase other than `PrOpened` (a merge
/// hold only means anything while the request is open — `merge_hold_detail`
/// gates the same way), no hold recorded at all, and `checks_waiting` still
/// within [`QUIET_HOLD_PASSES`].
pub(super) fn held_reason(ledger: &Ledger, agent_id: &str) -> Option<String> {
    let entry = ledger
        .entries
        .iter()
        .find(|entry| entry.agent_id == agent_id)?;
    if entry.phase != LedgerPhase::PrOpened {
        return None;
    }
    if let Some(reason) = entry.approval_dropped.as_deref() {
        return Some(sentence(reason));
    }
    if let Some(pending) = entry.merge_recovery.pending.as_ref() {
        return Some(sentence(&format!(
            "merge_recovery_pending:{}",
            pending.reason
        )));
    }
    let reason = entry.merge_hold.reason.as_deref()?;
    if reason == CHECKS_WAITING && entry.merge_hold.passes < QUIET_HOLD_PASSES {
        return None;
    }
    Some(sentence(reason))
}

fn sentence(reason: &str) -> String {
    humanize::static_sentence(reason)
        .unwrap_or(reason)
        .to_string()
}

#[cfg(test)]
#[path = "hold_reason_tests.rs"]
mod tests;
