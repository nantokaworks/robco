//! Whether the merge judge allows one pull request through — and when to start
//! asking, which is before the deterministic gate has cleared it.
//!
//! Split out to keep the caller under its size limit: the judge call, its
//! fail-safe handling, and the veto/escalate mapping are one cohesive unit that
//! `merge_evaluate::evaluate` only needs to invoke, not inline.

use serde_json::Value;

use super::{
    merge_decision::{Halt, log},
    merge_gate, merge_hold_recheck, merge_judge_fail_safe,
    merge_queue::Heads,
    merge_state, pull_request,
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

/// Starts the merge judgment for a pull request that cannot merge on this pass,
/// so the verdict is already in hand on the one where it can.
///
/// The judge round trip is minutes of model time. Asking for it only after the
/// checks go green made every merge cost a full check run *plus* a full judgment,
/// one after the other, when the two have no dependency on each other at all: the
/// judge reads the change, and a pull request waiting on its checks — or one whose
/// branch was just updated onto its base — has the same change it will have when
/// the gate clears. Running them together is what the merge queue was missing.
///
/// The caller decides *when* priming makes sense — see [`waiting_on_progress`]
/// and the settling wait in `merge_evaluate::evaluate`. Three bounds apply here:
/// one pull request per repository per pass ([`Heads::claim_judge`]), a per-entry
/// budget (`overseer.max_merge_judge_primes`), and the autonomy envelope, which
/// refuses a change the judge would never be asked about anyway.
pub(super) fn prime(
    entry: &mut LedgerEntry,
    url: &str,
    value: &Value,
    config: &Config,
    judgments: &mut JudgmentQueue,
    consecutive_failures: u32,
    heads: &mut Heads,
) -> Result<()> {
    let facts = change_facts(value, consecutive_failures, judgments.llm_calls_today());
    // The same envelope question `judge_allows` asks, and the honest one to ask
    // here: protection really is verified — the gate checks it before any wait
    // that reaches this — and green checks are what the pull request is waiting
    // for, so assuming them asks "once this clears, does the judge get to
    // decide?". A change the envelope escalates never reaches the judge at all,
    // so a session for it would buy a verdict nobody asks for. Asked before the
    // slot is claimed, so such a change does not spend its repository's one
    // priming slot on a judgment it was never going to start. Nothing merges on
    // this answer either way; the gate still decides that.
    if judgment_after_gate(true, true, &facts, config, || ()).is_none() {
        return Ok(());
    }
    // Budget first: `claim_judge` mutates, so asking it before the budget would
    // let a pull request whose own budget is spent take its repository's priming
    // slot every pass and starve the one behind it of the overlap entirely.
    if !budget_left(entry, config) || !heads.claim_judge(&entry.repo) {
        return Ok(());
    }
    // Charged on what was bought, not on what was asked for. A pull request held
    // on the same wait for several passes primes the same question every pass,
    // and `prime_merge` answers `false` for all but the first — charging those
    // would spend the whole budget on one judgment and leave nothing for the
    // genuinely new question a later push raises.
    if judgments.prime_merge(merge_case(entry, url, value))? {
        entry.merge_judge_primes = entry.merge_judge_primes.saturating_add(1);
    }
    Ok(())
}

/// Whether `entry` may still start an early judgment.
///
/// The budget exists because a judgment is keyed on the change, not on the pull
/// request: every push that moves the diff is a new question, and `checks_waiting`
/// follows every push. Without a bound, a worker pushing ten CI fixes would buy
/// ten judgments where waiting for green checks bought one — and `daily_llm_budget`
/// running out does not merely stop priming, it escalates every merge on the board
/// for the rest of the day (`autonomy::RiskCategory::BudgetExceeded`).
///
/// Only the *early* judgment is bounded. A pull request that spends this budget
/// still reaches the judge the ordinary way through [`judge_allows`] once its gate
/// clears; all it loses is the overlap.
fn budget_left(entry: &LedgerEntry, config: &Config) -> bool {
    entry.merge_judge_primes < config.overseer.max_merge_judge_primes
}

/// Whether a hold reason means the merge is still coming.
///
/// Both name work that finishes on its own: a check run that has not reported,
/// and a branch just updated onto its base so its checks re-run. Every other exit
/// of the gate names either a fault someone has to fix or a wait on another pull
/// request — and for that second group the pull request actually ahead in the
/// queue is the one worth a judgment, not this one.
pub(super) fn waiting_on_progress(reason: &str) -> bool {
    reason == merge_gate::CHECKS_WAITING || reason == merge_state::BRANCH_UPDATED
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
