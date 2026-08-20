//! One repository's turn through the auto-merge pass.
//!
//! Split out of `merge` so that module holds only the pass-wide bookkeeping —
//! settling the barrier map, grouping entries by repository, running the
//! bounded worker pool — while this one holds the sequential walk through a
//! single repository's own queue of entries. That walk is the same one
//! `merge::auto_merge_pass` used to run directly against every entry in the
//! ledger; it is unchanged here in order or effect, just scoped to one
//! repository and run on its own worker thread alongside every other
//! repository's. See `merge_concurrency` for the pool that calls this.
//!
//! `entries` and `settling` arrive owned by [`RepoWork`], extracted from the
//! ledger and the settling map before any worker thread starts — so nothing
//! this function touches is shared with another repository's run of it. The
//! one thing that genuinely is shared — the protection cache — is passed in
//! already synchronised (`ProtectionCache` is internally locked).

use super::{
    merge_decision::{Halt, Outcome, log, log_halt, manual_skip},
    merge_evaluate::evaluate,
    merge_hold::{self, HoldPlan},
    merge_hold_recheck, merge_queue, merge_recovery,
    merge_settle::{self, Barrier},
    protection::ProtectionCache,
};
use std::time::{Duration, Instant};

use crate::{
    Result,
    config::Config,
    overseer::{
        ledger::{LedgerEntry, LedgerPhase, MergeSettling},
        logging::{self, DecisionKind},
    },
    registry::Registry,
};

/// Reason recorded for a merge candidate whose pull request the ledger never
/// learned. There is nothing to read, so the gate stops before every other step.
const MISSING_PR_URL: &str = "missing_pr_url";

/// One repository's ledger entries, plus the barrier state carried over from
/// the last pass.
pub(super) struct RepoWork<'a> {
    pub(super) repo: String,
    pub(super) entries: Vec<&'a mut LedgerEntry>,
    pub(super) settling: Option<MergeSettling>,
}

/// What one repository's evaluation did, handed back to `auto_merge_pass` to
/// fold into the pass-wide state once every repository's worker has finished.
pub(super) struct RepoOutcome {
    pub(super) repo: String,
    pub(super) settling: Option<MergeSettling>,
    /// Whether any entry in this repository merged this pass. Folded into
    /// `Ledger.counters.consecutive_failures` by the caller, since that
    /// counter is shared across every repository rather than kept per-repo.
    pub(super) merged: bool,
    /// How long this repository's own evaluation took — see
    /// `merge_pass_telemetry`, which uses the slowest of these across the
    /// whole pass to name the repository actually worth an operator's look.
    pub(super) duration: Duration,
}

pub(super) fn run(
    mut work: RepoWork,
    config: &Config,
    cache: &ProtectionCache,
    registry: &Registry,
    max_rechecks: u32,
    max_settle_passes: u32,
) -> Result<RepoOutcome> {
    let started = Instant::now();
    // Fresh every pass, and scoped to this repository alone: which pull
    // request is this repository's head of queue is recomputed from
    // iteration order each time, never remembered from the last pass or
    // shared with any other repository's worker. See `merge_queue`.
    let mut heads = merge_queue::Heads::new();
    let mut merged = false;
    for entry in work.entries.iter_mut() {
        // Read, never charged here: the budget pays for a pass that re-read the
        // gate and found it still holding, and this pass has not reached the gate
        // yet. Charging on the way in spends it on outcomes the budget is not for
        // — most of all on a pass that clears the gate and only waits on a
        // judgment, which arrives once and is not a condition to re-check.
        let recheck = merge_hold_recheck::due(entry, max_rechecks);
        // An operator-granted override earns its own look even when the
        // hold-cap budget would not otherwise grant one — a live
        // `operator_override` never enters that budget on its own, so
        // without this an escalated entry carrying one would sit parked
        // forever the same way it does without one. See
        // `merge_allow::take_operator_override`.
        let reconsidering =
            entry.phase == LedgerPhase::Escalated && (recheck || entry.operator_override.is_some());
        if entry.phase != LedgerPhase::PrOpened && !reconsidering {
            continue;
        }
        // dropr:500: only a pull request the operator actually asked about —
        // a live `merge_approval` (TUI `m`, Discord `!merge`) or
        // `operator_override` (`robco_approve`'s no-live-session fallback) —
        // is ever looked at. Nothing here yet for a `PrOpened` entry with
        // neither: no gate, no management check, no decision-log entry.
        // `reconsidering` above already proves a request existed for an
        // escalated entry reaching this point.
        if entry.phase == LedgerPhase::PrOpened
            && entry.merge_approval.is_none()
            && entry.operator_override.is_none()
        {
            continue;
        }
        // The management check is not the phase check. An entry the phase check
        // drops is not a merge candidate and there is nothing to say about it;
        // an entry whose worker is manual *is* a candidate Overseer is declining
        // to act on, and taking that silently left the operator unable to tell
        // "Overseer decided not to merge this" from "the merge pass never ran".
        let auto = worker_is_auto(entry, registry);
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
        let settling = match merge_settle::barrier(&mut work.settling, max_settle_passes) {
            Barrier::Open => false,
            Barrier::Held if heads.free(&entry.repo) => true,
            Barrier::Held => {
                log(entry, DecisionKind::Hold, merge_settle::SETTLING, "")?;
                continue;
            }
            // The pull never landed within its bound. Merging anyway is the
            // lesser failure — a repository parked forever needs an operator
            // either way — but it is recorded under its own reason so the log
            // says the base was never confirmed.
            Barrier::Lifted => {
                log(
                    entry,
                    DecisionKind::Hold,
                    merge_settle::SETTLE_CAP_REACHED,
                    "",
                )?;
                false
            }
        };
        let Some(url) = entry.pr_url.clone() else {
            // The gate stops before it can read a revision, so the budget is keyed
            // on the reason alone. It still ends the repetition, which is the only
            // thing an entry with no pull request ever produced.
            hold(entry, &Halt::hold(MISSING_PR_URL), "", "", config, registry)?;
            continue;
        };
        let phase_before = entry.phase;
        let outcome = evaluate(entry, &url, config, cache, registry, &mut heads, settling)?;
        match outcome {
            Outcome::Merged => {
                merged = true;
                merge_hold::cleared(entry);
                merge_hold_recheck::settle(entry);
                merge_settle::begin(&mut work.settling);
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
                        &head,
                    )?;
                }
                hold(entry, &halt, &head, &base, config, registry)?;
            }
            // Recorded, not charged: the gate is no longer what holds this
            // entry, and the repository's own post-merge pull is not a
            // condition an entry can escalate its way out of.
            Outcome::Settling => {
                log(entry, DecisionKind::Hold, merge_settle::SETTLING, "")?;
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
    Ok(RepoOutcome {
        repo: work.repo,
        settling: work.settling,
        merged,
        duration: started.elapsed(),
    })
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
            log_halt(entry, halt, head, overseer.protection_mode)?;
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
                head,
            )
        }
        HoldPlan::Spent => Ok(()),
    }
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
#[path = "merge_repo_pass_tests.rs"]
mod tests;
