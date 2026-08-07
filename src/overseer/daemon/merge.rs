use std::collections::HashSet;

use super::{
    merge_decision::{Halt, Outcome, log, log_halt, manual_skip},
    merge_evaluate::evaluate,
    merge_hold::{self, HoldPlan},
    merge_hold_recheck, merge_queue, merge_recovery, merge_settle,
    merge_settle::Barrier,
    protection::ProtectionCache,
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
    // Fresh every pass, and shared across every entry it evaluates this pass: which
    // pull request is a repository's head of queue is recomputed from iteration
    // order each time, never remembered from the last pass. See `merge_queue`.
    let mut heads = merge_queue::Heads::new();
    for entry in entries.iter_mut() {
        // Read, never charged here: the budget pays for a pass that re-read the
        // gate and found it still holding, and this pass has not reached the gate
        // yet. Charging on the way in spends it on outcomes the budget is not for
        // — most of all on a pass that clears the gate and only waits on a
        // judgment, which arrives once and is not a condition to re-check.
        let recheck = merge_hold_recheck::due(entry, max_rechecks);
        // An operator-granted bypass earns its own look even when neither the
        // judge queue nor the hold-cap budget would otherwise grant one — the
        // autonomy envelope's own hard stop never enters either, so without
        // this an envelope-escalated entry with a pending override would sit
        // parked forever the same way it does without one. See
        // `merge_judge_gate::take_operator_override`.
        let reconsidering = entry.phase == LedgerPhase::Escalated
            && (judgments.has_terminal_merge(&entry.task_id, entry.pr_url.as_deref())
                || recheck
                || entry.operator_override.is_some());
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
        // The barrier guards the merge — a base the primary worktree has not
        // pulled yet — not the steps before it. The pull request now at the head
        // of this repository's queue has to catch up to the base that merge just
        // advanced, and `gh pr update-branch` runs entirely on GitHub's side, so
        // holding the whole evaluation cost the head a poll interval it did not
        // owe.
        //
        // The `free` guard stops once one entry of the repository has actually
        // claimed the slot. It does not bound the pass to a single read: an entry
        // that halts before `merge_state_cleared` — a red check, a conflict, an
        // unresolved prerequisite — never claims, so the entry behind it is read
        // too, which is the same order the barrier-open path already walks to
        // find the head. What it does bound is the tail *behind* a real head,
        // which would otherwise each spend a `gh pr view` to learn they must
        // wait.
        let settling = match merge_settle::barrier(merge_settling, &entry.repo, max_settle_passes) {
            Barrier::Open => false,
            Barrier::Held if heads.free(&entry.repo) => true,
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
                false
            }
        };
        let Some(url) = entry.pr_url.clone() else {
            // The gate stops before it can read a revision, so the budget is keyed
            // on the reason alone. It still ends the repetition, which is the only
            // thing an entry with no pull request ever produced.
            hold(
                entry,
                &Halt::hold(MISSING_PR_URL),
                "",
                "",
                config,
                &registry,
            )?;
            continue;
        };
        let phase_before = entry.phase;
        let outcome = evaluate(
            entry,
            &url,
            config,
            cache,
            &registry,
            judgments,
            consecutive_failures,
            &mut heads,
            settling,
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
            Outcome::Halted { halt, head, base } => {
                if recheck && merge_hold_recheck::charge(entry, &halt.reason, &head, max_rechecks) {
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
                hold(entry, &halt, &head, &base, config, &registry)?;
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
            // Recorded, not charged, for the same reason `Pending` is not: the
            // gate is no longer what holds this entry, and the repository's own
            // post-merge pull is not a condition an entry can escalate its way
            // out of.
            Outcome::Settling => {
                log(entry, DecisionKind::Hold, merge_settle::SETTLING)?;
                merge_hold::cleared(entry);
            }
        }
        // The head slot belongs to whoever is still in this repository's queue.
        // An entry that merged or escalated on this pass has left it, so the
        // pull request behind it takes the slot now — and starts its own branch
        // update in this same pass — rather than a poll interval from now.
        // `release` ignores a caller that is not the recorded holder, which most
        // entries reaching a terminal phase here are not.
        if entry.phase != phase_before && super::terminal(entry.phase) {
            heads.release(&entry.repo, &entry.agent_id);
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
    base: &str,
    config: &Config,
    registry: &Registry,
) -> Result<()> {
    match merge_hold::charge(entry, halt, head, config.overseer.max_merge_holds) {
        HoldPlan::Record => {
            let overseer = &config.overseer;
            let language = config.language.as_deref();
            log_halt(entry, halt, overseer.protection_mode)?;
            merge_recovery::consider(
                entry,
                &halt.reason,
                head,
                base,
                overseer,
                registry,
                language,
            )
        }
        HoldPlan::CapReached => {
            entry.phase = LedgerPhase::Escalated;
            entry.worker_escalated = false;
            merge_hold_recheck::escalated(entry, &halt.reason, head);
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
