//! Whether the merge judge allows one pull request through, once the
//! deterministic gate has already cleared it.
//!
//! Split out of `merge` to keep that file under its size limit: the judge call,
//! its fail-safe handling, and the veto/escalate mapping are one cohesive unit
//! that `merge::evaluate` only needs to invoke, not inline.

use serde_json::Value;

use super::{merge_decision::Halt, merge_hold_recheck, merge_judge_fail_safe};
use crate::{
    Result,
    config::Config,
    overseer::{
        autonomy::{Decision, merge_envelope_decision},
        judge::{JudgmentQueue, MergeJudgment, change_facts, judgment_after_gate, merge_case},
        ledger::{LedgerEntry, LedgerPhase},
    },
};

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
    let Some(advice) =
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
    let Some(advice) = advice? else {
        return Ok(Judgment::Queued);
    };
    if merge_judge_fail_safe::handle(entry, &advice, &config.overseer)? {
        return Ok(Judgment::Queued);
    }
    // A real verdict means the judge, via `has_terminal_merge`, is now the
    // authority reconsidering this entry — so a hold-cap recheck budget left
    // over from before the deterministic gate cleared must not linger and get
    // misattributed to whatever this verdict decides next.
    merge_hold_recheck::settle(entry);
    let allows_merge = judgment_allows_merge(entry, advice.outcome);
    match advice.outcome {
        MergeJudgment::Allow => {}
        MergeJudgment::Veto => {
            return Ok(Judgment::Halt(Halt::escalate(format!(
                "judge_veto:{}",
                advice.reason
            ))));
        }
        MergeJudgment::Escalate => {
            return Ok(Judgment::Halt(Halt::escalate(format!(
                "judge_escalate:{}",
                advice.reason
            ))));
        }
    }
    debug_assert!(allows_merge);
    Ok(Judgment::Allow)
}

fn judgment_allows_merge(entry: &mut LedgerEntry, outcome: MergeJudgment) -> bool {
    if outcome == MergeJudgment::Allow {
        true
    } else {
        entry.phase = LedgerPhase::Escalated;
        entry.worker_escalated = false;
        false
    }
}

#[cfg(test)]
#[path = "merge_judge_gate_tests.rs"]
mod tests;
