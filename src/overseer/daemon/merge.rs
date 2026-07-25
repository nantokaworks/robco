use std::{collections::HashSet, process::Command};

use serde_json::Value;

use super::{
    COMMAND_TIMEOUT,
    merge_decision::{Halt, Outcome, log, log_gated, log_halt, manual_skip},
    merge_recovery, merge_state,
    merge_state::{BehindPlan, MergeState},
    protection,
    protection::ProtectionCache,
    pull_request::{self, Checks, base_branch, checks, head_sha},
};
use crate::{
    Result,
    config::Config,
    overseer::{
        autonomy::{Decision, merge_envelope_decision},
        config::ProtectionMode,
        exec::run_timeout,
        judge::{JudgmentQueue, MergeJudgment, change_facts, judgment_after_gate, merge_case},
        ledger::{Ledger, LedgerEntry, LedgerPhase},
        logging::{self, DecisionKind},
    },
    registry::Registry,
};

/// Reason recorded for a pull request skipped because another one merged into the same
/// base during this pass.
const REPO_ALREADY_MERGED: &str = "repo_merged_this_pass";

/// Reason recorded when a check finished and did not pass — the worker's own head is red.
const CHECKS_FAILED: &str = "checks_not_green";

/// Reason recorded when the checks have not finished, or the pull request is no longer
/// open. Nothing has failed, so nothing is handed back.
const CHECKS_WAITING: &str = "checks_waiting";

pub(super) fn auto_merge_pass(
    config: &Config,
    ledger: &mut Ledger,
    cache: &mut ProtectionCache,
    judgments: &mut JudgmentQueue,
) -> Result<()> {
    if !config.overseer.auto_merge {
        return Ok(());
    }
    let registry = Registry::load()?;
    let consecutive_failures = ledger.counters.consecutive_failures;
    // Merges are serialised per repository: a merge advances the base and leaves every
    // other pull request of that repository behind, so their reads from earlier in this
    // pass no longer describe a mergeable branch. Other repositories stay independent.
    let mut merged_repos: HashSet<String> = HashSet::new();
    for entry in ledger.entries.iter_mut() {
        let reconsidering = entry.phase == LedgerPhase::Escalated
            && judgments.has_terminal_merge(&entry.task_id, entry.pr_url.as_deref());
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
        if merged_repos.contains(&entry.repo) {
            log(entry, DecisionKind::Hold, REPO_ALREADY_MERGED)?;
            continue;
        }
        let Some(url) = entry.pr_url.clone() else {
            log(entry, DecisionKind::Hold, "missing_pr_url")?;
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
                ledger.counters.consecutive_failures = 0;
                merged_repos.insert(entry.repo.clone());
            }
            Outcome::Halted { halt, head } => {
                log_halt(entry, &halt, config.overseer.protection_mode)?;
                merge_recovery::consider(entry, &halt.reason, &head, &config.overseer, &registry)?;
            }
            Outcome::Pending => {}
        }
    }
    Ok(())
}

/// Runs one pull request through the gate: read, protection, checks, merge state,
/// merge judge, merge. Every non-merge exit names itself, so the caller has one
/// place to record the decision and one place to decide whether the failure is
/// the owning worker's to fix.
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
    let mode = config.overseer.protection_mode;
    if let Some(unmet) =
        protection::unmet_condition(entry, registry, cache, mode, base_branch(&value))
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
    Ok(
        match merge_now(entry, url, &config.overseer.merge_strategy, mode)? {
            Ok(()) => Outcome::Merged,
            Err(halt) => halt.on(&head),
        },
    )
}

/// What the merge judge said about a pull request the deterministic gate cleared.
enum Judgment {
    Allow,
    /// The judge, or the autonomy envelope, stopped the merge under this reason.
    Halt(Halt),
    /// No verdict yet: the judgment is queued, so the pull request simply waits.
    Queued,
}

/// Puts a pull request the deterministic gate cleared to the merge judge.
fn judge_allows(
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

/// Acts on GitHub's own mergeability verdict. Returns `None` when the merge may proceed.
///
/// A branch that has merely fallen behind its base is updated and returned to the queue
/// so its required checks re-run against the new head; it is a recoverable state, so it
/// never marks the entry failed. Every other non-mergeable state is held under a reason
/// naming the state itself.
fn merge_state_cleared(
    entry: &mut LedgerEntry,
    url: &str,
    value: &Value,
    config: &Config,
) -> Option<Halt> {
    match merge_state::merge_state(value) {
        MergeState::Ready => None,
        MergeState::Held(raw) => Some(Halt::hold(merge_state::hold_reason(raw))),
        MergeState::Behind => match merge_state::plan_update(entry, &config.overseer) {
            BehindPlan::Update(flag) => {
                Some(match merge_state::run_update(&entry.repo, url, flag) {
                    Ok(()) => Halt::hold(merge_state::BRANCH_UPDATED),
                    Err(reason) => Halt::hold(reason),
                })
            }
            BehindPlan::Escalate => {
                entry.phase = LedgerPhase::Escalated;
                Some(Halt::escalate(merge_state::UPDATE_CAP_REACHED))
            }
        },
    }
}

/// Merges the pull request, recording the merge itself once GitHub accepted it.
///
/// The merge is the one decision recorded here rather than by the caller: it is the
/// only outcome that is not a failure, so it never reaches the recovery step.
fn merge_now(
    entry: &mut LedgerEntry,
    url: &str,
    merge_strategy: &str,
    mode: ProtectionMode,
) -> Result<std::result::Result<(), Halt>> {
    let strategy = match merge_strategy {
        "merge" => "--merge",
        "rebase" => "--rebase",
        _ => "--squash",
    };
    let mut merge = Command::new("gh");
    merge
        .current_dir(&entry.repo)
        .args(["pr", "merge", url, strategy]);
    Ok(match run_timeout(merge, COMMAND_TIMEOUT) {
        Ok(output) if output.status.success() => {
            entry.phase = LedgerPhase::Merged;
            log_gated(
                entry,
                DecisionKind::Merge,
                strategy.trim_start_matches("--"),
                mode,
            )?;
            Ok(())
        }
        Ok(output) => Err(Halt::hold(format!("merge_exit:{}", output.status))),
        Err(error) => Err(Halt::hold(format!("merge_error:{error}"))),
    })
}

fn judgment_allows_merge(entry: &mut LedgerEntry, outcome: MergeJudgment) -> bool {
    if outcome == MergeJudgment::Allow {
        true
    } else {
        entry.phase = LedgerPhase::Escalated;
        false
    }
}

fn worker_is_auto(entry: &LedgerEntry, registry: &Registry) -> bool {
    registry
        .repos
        .iter()
        .flat_map(|repo| &repo.agents)
        .find(|agent| agent.id == entry.agent_id)
        .is_none_or(|agent| agent.management == crate::model::ManagementMode::Auto)
}

#[cfg(test)]
#[path = "../judge/merge_tests.rs"]
mod tests;
