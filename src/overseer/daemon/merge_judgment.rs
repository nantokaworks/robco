//! What the merge judge said about a pull request the deterministic gate cleared.
//!
//! Kept apart from the pass itself because reading a verdict is not the same
//! machine as running the gate. Three of the four things that can come back —
//! a queued judgment, a refusal the gate already holds, and a session that
//! failed instead of answering — are not statements about the change at all,
//! and each has to leave the pull request somewhere a later pass can still
//! reach it. Collapsing them into "no verdict yet" is what parked a green,
//! mergeable pull request behind an expired auth token with no decision
//! recorded on any pass.

use serde_json::Value;

use super::merge_decision::Halt;
use crate::{
    Result,
    config::Config,
    overseer::{
        autonomy::{Decision, merge_envelope_decision},
        judge::{
            JudgmentQueue, MergeAdvice, MergeJudgment, MergeVerdict, change_facts,
            judgment_after_gate, merge_case,
        },
        ledger::{LedgerEntry, LedgerPhase},
    },
};

/// Reason recorded when the judge's own session failed instead of answering.
///
/// Deliberately not one of the reasons `merge_recovery::classify` calls
/// recoverable: the worker cannot fix an expired token or a session that never
/// wrote its result, so handing the failure to it would spend a worker turn and
/// return the entry to the phase it was already in.
const JUDGE_UNAVAILABLE: &str = "judge_unavailable";

/// Reason recorded when the re-asks are spent. It carries the last failure,
/// because "the judge never answered" is only actionable together with what it
/// failed on.
const JUDGE_UNAVAILABLE_CAP: &str = "judge_unavailable_cap_reached";

/// Reason recorded on a pass that reconsidered an escalated entry and found the
/// judge's refusal still standing.
///
/// The refusal itself was recorded when it was given, and the words the judge
/// used are not kept, so this says only that nothing has changed since. That is
/// the whole point: the pass that declines to act is now a decision an operator
/// can see rather than the silence that reads as a dead auto-merge.
const JUDGE_VERDICT_STANDS: &str = "judge_verdict_stands";

/// What the merge judge said about a pull request the deterministic gate cleared.
pub(super) enum Judgment {
    Allow,
    /// The judge, or the autonomy envelope, stopped the merge under this reason.
    Halt(Halt),
    /// No verdict yet: the judgment is queued, so the pull request simply waits.
    Queued,
}

/// Puts a pull request the deterministic gate cleared to the merge judge.
pub(super) fn judge_allows(
    entry: &mut LedgerEntry,
    url: &str,
    value: &Value,
    config: &Config,
    judgments: &mut JudgmentQueue,
    consecutive_failures: u32,
) -> Result<Judgment> {
    let facts = change_facts(value, consecutive_failures, judgments.llm_calls_today());
    let case = merge_case(entry, url, value);
    let Some(verdict) =
        judgment_after_gate(true, true, &facts, config, || judgments.merge_advice(case))
    else {
        if matches!(
            merge_envelope_decision(true, true, &facts, &config.overseer),
            Decision::Escalate(_)
        ) {
            return Ok(Judgment::Halt(Halt::escalate("autonomy_envelope")));
        }
        return Ok(Judgment::Queued);
    };
    Ok(match verdict? {
        MergeVerdict::Queued => Judgment::Queued,
        // The entry only reaches this gate again because its refusal is
        // remembered, and the refusal is what keeps it from merging. Recording
        // the pass costs one hold, which is bounded, and buys back the ability
        // to tell "Overseer is standing on a verdict" from "the merge pass
        // stopped running".
        MergeVerdict::Refused => Judgment::Halt(Halt::hold(JUDGE_VERDICT_STANDS)),
        MergeVerdict::Advice(advice) if advice.fail_safe => {
            unavailable(entry, &advice, config.overseer.max_judge_retries)
        }
        MergeVerdict::Advice(advice) => decided(entry, &advice),
    })
}

/// A judgment session that failed rather than answered.
///
/// The entry keeps its phase, so the next pass re-enters the gate and asks
/// again — a transient failure of the judge's own machinery is not a verdict
/// about the change, and the pull request must not be parked on one. The
/// re-asks are bounded because each costs a model session: past the bound the
/// entry escalates for real, which is where a judge that is not coming back
/// belongs.
fn unavailable(entry: &mut LedgerEntry, advice: &MergeAdvice, max_retries: u32) -> Judgment {
    entry.merge_hold.judge_failures = entry.merge_hold.judge_failures.saturating_add(1);
    if entry.merge_hold.judge_failures > max_retries {
        entry.phase = LedgerPhase::Escalated;
        return Judgment::Halt(Halt::escalate(format!(
            "{JUDGE_UNAVAILABLE_CAP}:{}",
            advice.reason
        )));
    }
    Judgment::Halt(Halt::hold(format!("{JUDGE_UNAVAILABLE}:{}", advice.reason)))
}

/// A verdict the judge actually gave.
///
/// It resets the judge-failure count whichever way it went: the session that
/// produced it worked, so the earlier failures were the transient thing the
/// count is there to survive rather than the broken judge it is there to bound.
fn decided(entry: &mut LedgerEntry, advice: &MergeAdvice) -> Judgment {
    entry.merge_hold.judge_failures = 0;
    match advice.outcome {
        MergeJudgment::Allow => Judgment::Allow,
        MergeJudgment::Veto => escalate(entry, "judge_veto", &advice.reason),
        MergeJudgment::Escalate => escalate(entry, "judge_escalate", &advice.reason),
    }
}

/// Takes the entry out of the merge pass on the judge's word, carrying the
/// judge's own reason so `decisions.jsonl` — and the handback the worker may
/// receive — say what it refused.
fn escalate(entry: &mut LedgerEntry, verdict: &str, reason: &str) -> Judgment {
    entry.phase = LedgerPhase::Escalated;
    Judgment::Halt(Halt::escalate(format!("{verdict}:{reason}")))
}

#[cfg(test)]
#[path = "merge_judgment_tests.rs"]
mod tests;
