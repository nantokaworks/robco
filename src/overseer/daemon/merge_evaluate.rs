//! One pull request's turn through the auto-merge gate.
//!
//! Split out of `merge` so that file holds the *pass* — which entries are
//! candidates, what each per-repository barrier says, and what an outcome does
//! to the ledger — while this one holds the sequence a single pull request runs
//! through: read, conclusion, dependency, gate, judge, merge. `merge_gate`
//! already holds the read-only middle of that sequence for the same reason.

use super::{
    merge_apply::merge_now,
    merge_concurrency::SharedJudgments,
    merge_decision::{self, Halt, Outcome},
    merge_dependency, merge_gate,
    merge_judge_gate::{Judgment, judge_allows, prime as prime_judge, waiting_on_progress},
    merge_queue,
    protection::ProtectionCache,
    pull_request::{self, base_sha, head_sha},
};
use crate::{Result, config::Config, overseer::ledger::LedgerEntry, registry::Registry};

/// Runs one pull request through the gate: read, conclusion, protection, merge
/// state, checks, merge state queue, merge judge, merge. Every non-merge exit
/// names itself, so the caller has one place to record the decision and one
/// place to decide whether the failure is the owning worker's to fix.
///
/// `settling` names a repository whose own last merge has not landed in the
/// primary worktree yet. It stops the merge and nothing else: every step before
/// the merge either reads GitHub or updates a branch on GitHub's side, and those
/// are exactly the steps the pull request now at the head of the queue has to
/// get through before it can merge at all.
#[allow(clippy::too_many_arguments)]
pub(super) fn evaluate(
    entry: &mut LedgerEntry,
    url: &str,
    config: &Config,
    cache: &ProtectionCache,
    registry: &Registry,
    judgments: &SharedJudgments,
    consecutive_failures: u32,
    heads: &mut merge_queue::Heads,
    settling: bool,
) -> Result<Outcome> {
    let value = match pull_request::read(&entry.repo, url) {
        Ok(value) => value,
        // The read failed, so there is no revision to attribute a failure to.
        Err(reason) => return Ok(Halt::hold(reason).on("", "")),
    };
    let head = head_sha(&value).to_owned();
    let base = base_sha(&value).to_owned();
    // A pull request GitHub no longer reports as open is a fact rather than
    // something to wait for, and it is read first because everything below costs
    // GitHub calls that cannot change the answer. The judge's terminal verdict is
    // dropped with it: that verdict is what keeps an escalated entry re-entering
    // this gate every pass, and a pull request that can never merge again has
    // nothing left to re-judge.
    if let Some(conclusion) = pull_request::conclusion(&value) {
        judgments
            .lock()
            .unwrap()
            .forget_terminal_merge(&entry.task_id, url)?;
        return Ok(merge_decision::concluded(entry, conclusion).on(&head, &base));
    }
    let dependency = merge_dependency::probe(&entry.task_id);
    if let Some(halt) = merge_gate::gate(
        entry, url, &value, config, cache, registry, heads, dependency,
    ) {
        // Started here rather than after the gate clears, because the wait the
        // gate just named is exactly the wait a judgment can run underneath.
        if waiting_on_progress(&halt.reason) {
            prime_judge(
                entry,
                url,
                &value,
                config,
                judgments,
                consecutive_failures,
                heads,
            )?;
        }
        return Ok(halt.on(&head, &base));
    }
    // Read before the judge, not after. `judge_allows` *consumes* a verdict that
    // has landed — and this pass would then drop it along with the merge, so the
    // next pass would pay for the same judgment a second time and wait for it
    // serially, which is the cost this module exists to remove. Priming instead
    // keeps the settling wait overlapped without taking an answer nobody here
    // can act on.
    if settling {
        prime_judge(
            entry,
            url,
            &value,
            config,
            judgments,
            consecutive_failures,
            heads,
        )?;
        return Ok(Outcome::Settling);
    }
    match judge_allows(entry, url, &value, config, judgments, consecutive_failures)? {
        Judgment::Allow => {}
        Judgment::Halt(halt) => return Ok(halt.on(&head, &base)),
        Judgment::Queued => return Ok(Outcome::Pending),
    }
    Ok(
        match merge_now(
            entry,
            url,
            config.merge_strategy,
            config.overseer.protection_mode,
        )? {
            Ok(()) => Outcome::Merged,
            Err(halt) => halt.on(&head, &base),
        },
    )
}
