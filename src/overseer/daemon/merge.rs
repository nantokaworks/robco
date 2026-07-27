use std::collections::HashSet;

use super::{
    merge_apply::{merge_now, merge_state_cleared},
    merge_decision::{self, Halt, Outcome, log, log_halt, manual_skip},
    merge_hold::{self, HoldPlan},
    merge_hold_recheck,
    merge_judge_gate::{Judgment, judge_allows},
    merge_recovery, merge_settle,
    merge_settle::Barrier,
    protection,
    protection::ProtectionCache,
    pull_request::{self, Checks, base_branch, checks, head_sha},
};
use crate::{
    Result,
    config::Config,
    overseer::{
        judge::JudgmentQueue,
        ledger::{Ledger, LedgerEntry, LedgerPhase},
        logging::{self, DecisionEntry, DecisionKind},
    },
    registry::Registry,
};

/// Reason recorded when a check finished and did not pass — the worker's own head is red.
const CHECKS_FAILED: &str = "checks_not_green";

/// Reason recorded when the checks have not finished. Nothing has failed, so nothing
/// is handed back.
const CHECKS_WAITING: &str = "checks_waiting";

/// Reason recorded for a merge candidate whose pull request the ledger never
/// learned. There is nothing to read, so the gate stops before every other step.
const MISSING_PR_URL: &str = "missing_pr_url";

pub(super) fn auto_merge_pass(
    config: &Config,
    ledger: &mut Ledger,
    cache: &mut ProtectionCache,
    judgments: &mut JudgmentQueue,
    pulled: &HashSet<String>,
) -> Result<()> {
    if !config.overseer.auto_merge {
        return Ok(());
    }
    let registry = Registry::load()?;
    let consecutive_failures = ledger.counters.consecutive_failures;
    // Merges are serialised per repository: a merge advances the base and leaves every
    // other pull request of that repository behind, so their reads from earlier in this
    // pass no longer describe a mergeable branch, and the primary worktree does not hold
    // the merge until the post-merge pull lands. The barrier outlives the pass because
    // that pull runs on a later one. Other repositories stay independent.
    let max_settle_passes = config.overseer.max_merge_settle_passes;
    // Field-wise borrows: the barrier is read and written while `entries` is iterated.
    let Ledger {
        entries,
        merge_settling,
        counters,
        ..
    } = ledger;
    for repo in merge_settle::settle(merge_settling, pulled) {
        log_repo(&repo, merge_settle::SETTLED)?;
    }
    merge_settle::age(merge_settling);
    let max_rechecks = config.overseer.max_merge_hold_rechecks;
    for entry in entries.iter_mut() {
        // Read, never charged here: the budget pays for a pass that re-read the
        // gate and found it still holding, and this pass has not reached the gate
        // yet. Charging on the way in spends it on outcomes the budget is not for
        // — most of all on a pass that clears the gate and only waits on a
        // judgment, which arrives once and is not a condition to re-check.
        let recheck = merge_hold_recheck::due(entry, max_rechecks);
        let reconsidering = entry.phase == LedgerPhase::Escalated
            && (judgments.has_terminal_merge(&entry.task_id, entry.pr_url.as_deref()) || recheck);
        if entry.phase != LedgerPhase::PrOpened && !reconsidering {
            continue;
        }
        // The management check is not the phase check. An entry the phase check
        // drops is not a merge candidate and there is nothing to say about it;
        // an entry whose worker is manual *is* a candidate Overseer is declining
        // to act on, and taking that silently left the operator unable to tell
        // "Overseer decided not to merge this" from "the merge pass never ran".
        let auto = worker_is_auto(entry, &registry);
        if let Some(skip) = manual_skip(entry, auto) {
            logging::append(&skip)?;
        }
        if !auto {
            continue;
        }
        match merge_settle::barrier(merge_settling, &entry.repo, max_settle_passes) {
            Barrier::Open => {}
            Barrier::Held => {
                log(entry, DecisionKind::Hold, merge_settle::SETTLING)?;
                continue;
            }
            // The pull never landed within its bound. Merging anyway is the
            // lesser failure — a repository parked forever needs an operator
            // either way — but it is recorded under its own reason so the log
            // says the base was never confirmed.
            Barrier::Lifted => {
                log(entry, DecisionKind::Hold, merge_settle::SETTLE_CAP_REACHED)?;
            }
        }
        let Some(url) = entry.pr_url.clone() else {
            // The gate stops before it can read a revision, so the budget is keyed
            // on the reason alone. It still ends the repetition, which is the only
            // thing an entry with no pull request ever produced.
            hold(entry, &Halt::hold(MISSING_PR_URL), "", config, &registry)?;
            continue;
        };
        let outcome = evaluate(
            entry,
            &url,
            config,
            cache,
            &registry,
            judgments,
            consecutive_failures,
        )?;
        match outcome {
            Outcome::Merged => {
                counters.consecutive_failures = 0;
                merge_hold::cleared(entry);
                merge_hold_recheck::settle(entry);
                merge_settle::begin(merge_settling, &entry.repo);
            }
            // The one outcome the recheck budget is for: this pass re-read the
            // gate and the gate still holds, so the look it was granted is spent.
            Outcome::Halted { halt, head } => {
                if recheck && merge_hold_recheck::charge(entry, max_rechecks) {
                    // Recorded on the pass that spends the last look, so the log
                    // says once — and only once — that nothing will reconsider
                    // this entry again. Without it the operator cannot tell an
                    // entry still being re-checked from one given up on.
                    log(
                        entry,
                        DecisionKind::Escalate,
                        &merge_hold_recheck::exhausted(&halt.reason),
                    )?;
                }
                hold(entry, &halt, &head, config, &registry)?;
            }
            // The deterministic gate cleared and only the judgment is outstanding,
            // so whatever this entry was last held on is no longer what is holding
            // it. Forgetting it here is what keeps a condition that came back after
            // clearing from inheriting the old condition's spent budget. The
            // recheck budget is deliberately not charged: the gate is no longer
            // what holds this entry, and a judgment round trip can outlast the
            // whole budget on a busy queue — spending it here would strand the
            // entry exactly the way this module exists to prevent.
            Outcome::Pending => merge_hold::cleared(entry),
        }
    }
    Ok(())
}

/// Records one held pass and charges it against the entry's hold budget.
///
/// Recovery is consulted only while the hold is still being recorded. Past the cap
/// the entry belongs to an operator, and a handback would return it to the phase it
/// just left — the escalation would undo itself on the pass that raised it.
fn hold(
    entry: &mut LedgerEntry,
    halt: &Halt,
    head: &str,
    config: &Config,
    registry: &Registry,
) -> Result<()> {
    match merge_hold::charge(entry, halt, head, config.overseer.max_merge_holds) {
        HoldPlan::Record => {
            let overseer = &config.overseer;
            let language = config.language.as_deref();
            log_halt(entry, halt, overseer.protection_mode)?;
            merge_recovery::consider(entry, &halt.reason, head, overseer, registry, language)
        }
        HoldPlan::CapReached => {
            entry.phase = LedgerPhase::Escalated;
            merge_hold_recheck::escalated(entry);
            log(
                entry,
                DecisionKind::Escalate,
                &merge_hold::cap_reached(&halt.reason),
            )
        }
        HoldPlan::Spent => Ok(()),
    }
}

/// Records a decision about a repository rather than about one of its entries.
///
/// The barrier coming down belongs to the repository — the merge that raised it
/// may have left the ledger by the time its pull lands — so there is no task id
/// to attribute it to, and inventing one from whichever entry happens to be
/// nearby would name a pull request that had nothing to do with it.
fn log_repo(repo: &str, reason: &str) -> Result<()> {
    let mut decision = DecisionEntry::new(DecisionKind::Hold, reason);
    decision.repo = Some(repo.to_owned());
    decision.source = Some("auto_merge".into());
    logging::append(&decision)
}

/// Runs one pull request through the gate: read, conclusion, protection, checks,
/// merge state, merge judge, merge. Every non-merge exit names itself, so the
/// caller has one place to record the decision and one place to decide whether
/// the failure is the owning worker's to fix.
fn evaluate(
    entry: &mut LedgerEntry,
    url: &str,
    config: &Config,
    cache: &mut ProtectionCache,
    registry: &Registry,
    judgments: &mut JudgmentQueue,
    consecutive_failures: u32,
) -> Result<Outcome> {
    let value = match pull_request::read(&entry.repo, url) {
        Ok(value) => value,
        // The read failed, so there is no revision to attribute a failure to.
        Err(reason) => return Ok(Halt::hold(reason).on("")),
    };
    let head = head_sha(&value).to_owned();
    // A pull request GitHub no longer reports as open is a fact rather than
    // something to wait for, and it is read first because everything below costs
    // GitHub calls that cannot change the answer. The judge's terminal verdict is
    // dropped with it: that verdict is what keeps an escalated entry re-entering
    // this gate every pass, and a pull request that can never merge again has
    // nothing left to re-judge.
    if let Some(conclusion) = pull_request::conclusion(&value) {
        judgments.forget_terminal_merge(&entry.task_id, url)?;
        return Ok(merge_decision::concluded(entry, conclusion).on(&head));
    }
    let mode = config.overseer.protection_mode;
    if let Some(unmet) =
        protection::unmet_condition(entry, registry, cache, mode, base_branch(&value))
        && !protection_gate_overridden(unmet, config.overseer.allow_unverifiable_protection)
    {
        return Ok(Halt::gated(format!("unprotected:{unmet}")).on(&head));
    }
    match checks(&value) {
        Checks::Green => {}
        Checks::Failed => return Ok(Halt::hold(CHECKS_FAILED).on(&head)),
        Checks::Waiting => return Ok(Halt::hold(CHECKS_WAITING).on(&head)),
    }
    if let Some(halt) = merge_state_cleared(entry, url, &value, config) {
        return Ok(halt.on(&head));
    }
    match judge_allows(entry, url, &value, config, judgments, consecutive_failures)? {
        Judgment::Allow => {}
        Judgment::Halt(halt) => return Ok(halt.on(&head)),
        Judgment::Queued => return Ok(Outcome::Pending),
    }
    Ok(match merge_now(entry, url, config.merge_strategy, mode)? {
        Ok(()) => Outcome::Merged,
        Err(halt) => halt.on(&head),
    })
}

/// Whether an `unprotected:*` reason should still gate the merge, given the
/// operator's `allow_unverifiable_protection` setting. Only `plan_unsupported` is
/// waivable — every other reason names a base branch the probe actually read and
/// found wanting, which this setting has no business overriding.
fn protection_gate_overridden(unmet: &str, allow_unverifiable_protection: bool) -> bool {
    unmet == protection::PLAN_UNSUPPORTED && allow_unverifiable_protection
}

/// A repo the Overseer does not manage should not have its pull requests merged
/// automatically either — the same silent-divergence risk `manual_skip` already
/// guards against per-worker, now checked per-repo before the per-worker read.
fn worker_is_auto(entry: &LedgerEntry, registry: &Registry) -> bool {
    let repo_auto = registry
        .repos
        .iter()
        .find(|repo| repo.path.to_string_lossy() == entry.repo)
        .is_none_or(|repo| repo.management == crate::model::ManagementMode::Auto);
    repo_auto
        && registry
            .repos
            .iter()
            .flat_map(|repo| &repo.agents)
            .find(|agent| agent.id == entry.agent_id)
            .is_none_or(|agent| agent.management == crate::model::ManagementMode::Auto)
}

#[cfg(test)]
#[path = "../judge/merge_tests.rs"]
mod tests;
