//! Bounding a merge judge that fails for an infrastructure reason.
//!
//! `MergeAdvice.fail_safe == true` means the judge session itself failed —
//! timed out, launched wrong, or exited without writing a result — and says
//! nothing about the change under review. `queue::queue_merges` does not cache
//! it as a terminal verdict, so the pull request is reconsidered on the next
//! pass instead of being parked under a verdict that was never about the
//! change. Re-asking is therefore the natural response to a transient
//! failure, but a judge that never recovers must still converge: this module
//! is the dedicated, bounded counter that stops the re-asks from running
//! forever and spending model budget in a loop.
//!
//! Kept apart from `merge_hold` and `merge_recovery` on purpose. Both key
//! their budgets on how a pull request last held (reason, head sha), which
//! resets every time the gate passes through a genuine `Outcome::Pending`
//! while a fresh judge session is in flight — exactly the state between one
//! fail-safe verdict and the next. A budget that resets itself every round
//! never converges, so this counter lives on the entry independently of both.

use crate::{
    Result,
    overseer::{
        config::OverseerConfig,
        judge::MergeAdvice,
        ledger::{LedgerEntry, LedgerPhase},
        logging::{self, DecisionEntry, DecisionKind},
    },
};

/// Reason recorded when the fail-safe re-ask budget is spent.
pub(super) const CAP_REACHED: &str = "merge_judge_fail_safe_cap_reached";

/// What to do about one fail-safe merge verdict.
enum Plan {
    /// Within budget: count the re-ask and let the next pass ask the judge again.
    ReAsk,
    /// The budget is spent: the entry escalates for real.
    CapReached,
}

/// Charges one fail-safe verdict to `entry`'s dedicated budget.
fn charge(entry: &mut LedgerEntry, max: u32) -> Plan {
    if entry.merge_judge_fail_safes >= max {
        Plan::CapReached
    } else {
        entry.merge_judge_fail_safes = entry.merge_judge_fail_safes.saturating_add(1);
        Plan::ReAsk
    }
}

/// Handles one merge verdict's fail-safe flag.
///
/// Returns `true` when `advice` was fail-safe — the caller's judgment is still
/// outstanding, so the pull request should wait for the next pass — bounding
/// the re-asks and escalating for real once the budget is spent. Returns
/// `false` for a real verdict, after resetting whatever fail-safe count a
/// judge that has since recovered left behind, so the caller judges it
/// normally.
///
/// Every fail-safe call logs — the re-ask itself while under budget, the
/// escalation once it is spent — so a pull request stuck on a broken judge is
/// never a silent pass the way an unbounded re-ask would be.
pub(super) fn handle(
    entry: &mut LedgerEntry,
    advice: &MergeAdvice,
    config: &OverseerConfig,
) -> Result<bool> {
    if !advice.fail_safe {
        entry.merge_judge_fail_safes = 0;
        return Ok(false);
    }
    match charge(entry, config.max_merge_judge_fail_safes) {
        Plan::ReAsk => log(
            entry,
            DecisionKind::Hold,
            &format!("judge_fail_safe:{}", advice.reason),
        )?,
        Plan::CapReached => {
            entry.phase = LedgerPhase::Escalated;
            log(entry, DecisionKind::Escalate, CAP_REACHED)?;
        }
    }
    Ok(true)
}

fn log(entry: &LedgerEntry, kind: DecisionKind, reason: &str) -> Result<()> {
    let mut decision = DecisionEntry::new(kind, reason);
    decision.task = Some(entry.task_id.clone());
    decision.repo = Some(entry.repo.clone());
    decision.pr_url = entry.pr_url.clone();
    decision.source = Some("merge_judge_fail_safe".into());
    decision.escalation_notify = super::merge_escalation::notify(kind, reason);
    logging::append(&decision)
}

#[cfg(test)]
#[path = "merge_judge_fail_safe_tests.rs"]
mod tests;
