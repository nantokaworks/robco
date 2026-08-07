//! Whether the merge judge allows one pull request through, once the
//! deterministic gate has already cleared it.
//!
//! Split out of `merge` to keep that file under its size limit: the judge call,
//! its fail-safe handling, and the veto/escalate mapping are one cohesive unit
//! that `merge::evaluate` only needs to invoke, not inline.

use serde_json::Value;

use super::{
    merge_decision::{Halt, log},
    merge_hold_recheck, merge_judge_fail_safe, pull_request,
};
use crate::{
    Result,
    config::Config,
    overseer::{
        autonomy::{Decision, merge_envelope_decision},
        judge::{JudgmentQueue, MergeJudgment, change_facts, judgment_after_gate, merge_case},
        ledger::{LedgerEntry, LedgerPhase},
        logging::DecisionKind,
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
    let head = pull_request::head_sha(value);
    let Some(advice) =
        judgment_after_gate(true, true, &facts, config, || judgments.merge_advice(case))
    else {
        if matches!(
            merge_envelope_decision(true, true, &facts, &config.overseer),
            Decision::Escalate(_)
        ) {
            if take_operator_override(entry, head, "autonomy_envelope")? {
                return Ok(Judgment::Allow);
            }
            return Ok(Judgment::Halt(Halt::escalate("autonomy_envelope")));
        }
        return Ok(Judgment::Queued);
    };
    let Some(advice) = advice? else {
        // Either still queued for its first verdict, or the judge was asked
        // again and simply reaffirmed the terminal verdict it remembers for
        // this pull request (`JudgmentQueue::merge_advice`'s
        // `terminal_merges.matches` short circuit). `has_terminal_merge` is
        // what actually tells the two apart — `entry.phase == Escalated`
        // alone would not: an entry the deterministic gate's own hold cap
        // escalated, never yet asked of the judge, is `Escalated` too, and
        // treating its first-ever queued query as an already-told verdict
        // would let an operator's bypass wave a change through with no judge
        // review at all. Only a pull request the judge has actually vetoed
        // or escalated before is eligible for the bypass here.
        if judgments.has_terminal_merge(&entry.task_id, Some(url))
            && take_operator_override(entry, head, "judge_verdict")?
        {
            return Ok(Judgment::Allow);
        }
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
    match advice.outcome {
        MergeJudgment::Allow => {}
        MergeJudgment::Veto => {
            let reason = format!("judge_veto:{}", advice.reason);
            if take_operator_override(entry, head, &reason)? {
                return Ok(Judgment::Allow);
            }
            judgment_allows_merge(entry, advice.outcome);
            return Ok(Judgment::Halt(Halt::escalate(reason)));
        }
        MergeJudgment::Escalate => {
            let reason = format!("judge_escalate:{}", advice.reason);
            if take_operator_override(entry, head, &reason)? {
                return Ok(Judgment::Allow);
            }
            judgment_allows_merge(entry, advice.outcome);
            return Ok(Judgment::Halt(Halt::escalate(reason)));
        }
    }
    Ok(Judgment::Allow)
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
fn take_operator_override(entry: &mut LedgerEntry, head: &str, bypassed: &str) -> Result<bool> {
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
    )?;
    Ok(true)
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
