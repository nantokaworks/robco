use std::{collections::HashSet, process::Command};

use serde_json::Value;

use super::{
    COMMAND_TIMEOUT, merge_state,
    merge_state::{BehindPlan, MergeState},
    protection,
    protection::ProtectionCache,
    pull_request::{self, base_branch, checks_green},
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
        logging::{self, DecisionEntry, DecisionKind},
    },
    registry::Registry,
};

/// Reason recorded for a pull request skipped because another one merged into the same
/// base during this pass.
const REPO_ALREADY_MERGED: &str = "repo_merged_this_pass";

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
        if (entry.phase != LedgerPhase::PrOpened && !reconsidering)
            || !worker_is_auto(entry, &registry)
        {
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
        let value = match pull_request::read(&entry.repo, &url) {
            Ok(value) => value,
            Err(reason) => {
                log(entry, DecisionKind::Hold, &reason)?;
                continue;
            }
        };
        let mode = config.overseer.protection_mode;
        if let Some(unmet) =
            protection::unmet_condition(entry, &registry, cache, mode, base_branch(&value))
        {
            log_gated(
                entry,
                DecisionKind::Hold,
                &format!("unprotected:{unmet}"),
                mode,
            )?;
            continue;
        }
        if !checks_green(&value) {
            log(entry, DecisionKind::Hold, "checks_not_green")?;
            continue;
        }
        if !merge_state_cleared(entry, &url, &value, config)? {
            continue;
        }
        if !judge_allows(entry, &url, &value, config, judgments, consecutive_failures)? {
            continue;
        }
        if merge_now(entry, &url, &config.overseer.merge_strategy, mode)? {
            ledger.counters.consecutive_failures = 0;
            merged_repos.insert(entry.repo.clone());
        }
    }
    Ok(())
}

/// Puts a pull request the deterministic gate cleared to the merge judge. Returns `true`
/// when the judge allows the merge; every other outcome records its own decision.
fn judge_allows(
    entry: &mut LedgerEntry,
    url: &str,
    value: &Value,
    config: &Config,
    judgments: &mut JudgmentQueue,
    consecutive_failures: u32,
) -> Result<bool> {
    let facts = change_facts(value, consecutive_failures, judgments.llm_calls_today());
    let case = merge_case(entry, url, value);
    let Some(advice) =
        judgment_after_gate(true, true, &facts, config, || judgments.merge_advice(case))
    else {
        if matches!(
            merge_envelope_decision(true, true, &facts, &config.overseer),
            Decision::Escalate(_)
        ) {
            log(entry, DecisionKind::Escalate, "autonomy_envelope")?;
        }
        return Ok(false);
    };
    let Some(advice) = advice? else {
        return Ok(false);
    };
    let allows_merge = judgment_allows_merge(entry, advice.outcome);
    match advice.outcome {
        MergeJudgment::Allow => {}
        MergeJudgment::Veto => {
            log(
                entry,
                DecisionKind::Escalate,
                &format!("judge_veto:{}", advice.reason),
            )?;
            return Ok(false);
        }
        MergeJudgment::Escalate => {
            log(
                entry,
                DecisionKind::Escalate,
                &format!("judge_escalate:{}", advice.reason),
            )?;
            return Ok(false);
        }
    }
    debug_assert!(allows_merge);
    Ok(true)
}

/// Acts on GitHub's own mergeability verdict. Returns `true` when the merge may proceed.
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
) -> Result<bool> {
    match merge_state::merge_state(value) {
        MergeState::Ready => return Ok(true),
        MergeState::Held(raw) => log(entry, DecisionKind::Hold, &merge_state::hold_reason(raw))?,
        MergeState::Behind => match merge_state::plan_update(entry, &config.overseer) {
            BehindPlan::Update(flag) => match merge_state::run_update(&entry.repo, url, flag) {
                Ok(()) => log(entry, DecisionKind::Hold, merge_state::BRANCH_UPDATED)?,
                Err(reason) => log(entry, DecisionKind::Hold, &reason)?,
            },
            BehindPlan::Escalate => {
                entry.phase = LedgerPhase::Escalated;
                log(
                    entry,
                    DecisionKind::Escalate,
                    merge_state::UPDATE_CAP_REACHED,
                )?;
            }
        },
    }
    Ok(false)
}

/// Merges the pull request. Returns `true` once GitHub accepted the merge.
fn merge_now(
    entry: &mut LedgerEntry,
    url: &str,
    merge_strategy: &str,
    mode: ProtectionMode,
) -> Result<bool> {
    let strategy = match merge_strategy {
        "merge" => "--merge",
        "rebase" => "--rebase",
        _ => "--squash",
    };
    let mut merge = Command::new("gh");
    merge
        .current_dir(&entry.repo)
        .args(["pr", "merge", url, strategy]);
    match run_timeout(merge, COMMAND_TIMEOUT) {
        Ok(output) if output.status.success() => {
            entry.phase = LedgerPhase::Merged;
            log_gated(
                entry,
                DecisionKind::Merge,
                strategy.trim_start_matches("--"),
                mode,
            )?;
            return Ok(true);
        }
        Ok(output) => log(
            entry,
            DecisionKind::Hold,
            &format!("merge_exit:{}", output.status),
        )?,
        Err(error) => log(entry, DecisionKind::Hold, &format!("merge_error:{error}"))?,
    }
    Ok(false)
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

fn log(entry: &LedgerEntry, kind: DecisionKind, reason: &str) -> Result<()> {
    logging::append(&decision(entry, kind, reason))
}

fn log_gated(
    entry: &LedgerEntry,
    kind: DecisionKind,
    reason: &str,
    mode: ProtectionMode,
) -> Result<()> {
    logging::append(&gated_decision(entry, kind, reason, mode))
}

/// Records the active strictness mode alongside the decision, so a merge that only
/// happened because the gate was loosened stays distinguishable in `decisions.jsonl`.
fn gated_decision(
    entry: &LedgerEntry,
    kind: DecisionKind,
    reason: &str,
    mode: ProtectionMode,
) -> DecisionEntry {
    let mut decision = decision(entry, kind, reason);
    decision.protection_mode = Some(mode.label().to_owned());
    decision
}

fn decision(entry: &LedgerEntry, kind: DecisionKind, reason: &str) -> DecisionEntry {
    let mut decision = DecisionEntry::new(kind, reason);
    decision.task = Some(entry.task_id.clone());
    decision.repo = Some(entry.repo.clone());
    decision.source = Some("auto_merge".into());
    decision
}

#[cfg(test)]
#[path = "../judge/merge_tests.rs"]
mod tests;
