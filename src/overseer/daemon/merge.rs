use std::process::Command;

use serde_json::Value;

use super::{COMMAND_TIMEOUT, protection, protection::ProtectionCache};
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

/// Base branch used when the pull request does not report one.
const DEFAULT_BASE_BRANCH: &str = "main";

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
    for entry in ledger.entries.iter_mut() {
        let reconsidering = entry.phase == LedgerPhase::Escalated
            && judgments.has_terminal_merge(&entry.task_id, entry.pr_url.as_deref());
        if (entry.phase != LedgerPhase::PrOpened && !reconsidering)
            || !worker_is_auto(entry, &registry)
        {
            continue;
        }
        let Some(url) = entry.pr_url.clone() else {
            log(entry, DecisionKind::Hold, "missing_pr_url")?;
            continue;
        };
        let mut view = Command::new("gh");
        view.current_dir(&entry.repo).args([
            "pr",
            "view",
            &url,
            "--json",
            "state,statusCheckRollup,title,body,files,additions,deletions,changedFiles,headRefOid,baseRefName",
        ]);
        let output = match run_timeout(view, COMMAND_TIMEOUT) {
            Ok(output) if output.status.success() => output,
            Ok(output) => {
                log(
                    entry,
                    DecisionKind::Hold,
                    &format!("check_probe_exit:{}", output.status),
                )?;
                continue;
            }
            Err(error) => {
                log(entry, DecisionKind::Hold, &format!("check_probe:{error}"))?;
                continue;
            }
        };
        let value: Value = match serde_json::from_slice(&output.stdout) {
            Ok(value) => value,
            Err(error) => {
                log(entry, DecisionKind::Hold, &format!("check_parse:{error}"))?;
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
        let facts = change_facts(&value, consecutive_failures, judgments.llm_calls_today());
        let case = merge_case(entry, &url, &value);
        let Some(advice) =
            judgment_after_gate(true, true, &facts, config, || judgments.merge_advice(case))
        else {
            if matches!(
                merge_envelope_decision(true, true, &facts, &config.overseer),
                Decision::Escalate(_)
            ) {
                log(entry, DecisionKind::Escalate, "autonomy_envelope")?;
            }
            continue;
        };
        let advice = advice?;
        let Some(advice) = advice else { continue };
        let allows_merge = judgment_allows_merge(entry, advice.outcome);
        match advice.outcome {
            MergeJudgment::Allow => {}
            MergeJudgment::Veto => {
                log(
                    entry,
                    DecisionKind::Escalate,
                    &format!("judge_veto:{}", advice.reason),
                )?;
                continue;
            }
            MergeJudgment::Escalate => {
                log(
                    entry,
                    DecisionKind::Escalate,
                    &format!("judge_escalate:{}", advice.reason),
                )?;
                continue;
            }
        }
        debug_assert!(allows_merge);
        let strategy = match config.overseer.merge_strategy.as_str() {
            "merge" => "--merge",
            "rebase" => "--rebase",
            _ => "--squash",
        };
        let mut merge = Command::new("gh");
        merge
            .current_dir(&entry.repo)
            .args(["pr", "merge", &url, strategy]);
        match run_timeout(merge, COMMAND_TIMEOUT) {
            Ok(output) if output.status.success() => {
                entry.phase = LedgerPhase::Merged;
                ledger.counters.consecutive_failures = 0;
                log_gated(
                    entry,
                    DecisionKind::Merge,
                    strategy.trim_start_matches("--"),
                    mode,
                )?;
            }
            Ok(output) => log(
                entry,
                DecisionKind::Hold,
                &format!("merge_exit:{}", output.status),
            )?,
            Err(error) => log(entry, DecisionKind::Hold, &format!("merge_error:{error}"))?,
        }
    }
    Ok(())
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

/// The pull request's base branch, which is the branch whose protection actually gates
/// the merge.
fn base_branch(value: &Value) -> &str {
    value
        .get("baseRefName")
        .and_then(Value::as_str)
        .filter(|branch| !branch.is_empty())
        .unwrap_or(DEFAULT_BASE_BRANCH)
}

pub(crate) fn checks_green(value: &Value) -> bool {
    if value.get("state").and_then(Value::as_str) != Some("OPEN") {
        return false;
    }
    let Some(checks) = value.get("statusCheckRollup").and_then(Value::as_array) else {
        return false;
    };
    !checks.is_empty()
        && checks.iter().all(|check| {
            check
                .get("conclusion")
                .or_else(|| check.get("state"))
                .and_then(Value::as_str)
                == Some("SUCCESS")
        })
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
