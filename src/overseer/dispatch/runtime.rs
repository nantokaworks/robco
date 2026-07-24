use chrono::{DateTime, Utc};
use std::collections::HashMap;

use super::{
    Candidate, plan_dispatch,
    worker::{SpawnOutcome, spawn_candidate},
};
use crate::overseer::{
    exec::COMMAND_TIMEOUT,
    judge::JudgmentQueue,
    ledger::Ledger,
    logging::{self, DecisionEntry, DecisionKind},
};
use crate::{Result, config::Config, dropr, registry::Registry};

/// `dropr task ready --limit` caps at 20; larger values make the CLI
/// exit with an argument error and the fetch fails for every repo.
const READY_FETCH_LIMIT: usize = 20;

pub fn dispatch_pass(
    config: &mut Config,
    ledger: &mut Ledger,
    now: DateTime<Utc>,
    judgments: &mut JudgmentQueue,
) -> Result<()> {
    let worker_modes = worker_modes()?;
    let preflight = plan_dispatch(&config.overseer, ledger, &[], now, &worker_modes);
    ledger.counters.date = Some(preflight.date);
    ledger.counters.dispatched_today = preflight.dispatched_today;
    if preflight.circuit_opened {
        open_circuit(config)?;
        return Ok(());
    }
    if let Some(decision) = preflight.decisions.first() {
        log_global(DecisionKind::Skip, &decision.reason)?;
        return Ok(());
    }

    let candidates = gather_candidates()?;
    let plan = plan_dispatch(&config.overseer, ledger, &candidates, now, &worker_modes);
    let approved = plan
        .decisions
        .iter()
        .filter(|decision| decision.dispatch)
        .filter_map(|decision| decision.candidate.clone())
        .collect::<Vec<_>>();
    let Some(advice) = judgments.dispatch_advice(&approved) else {
        return Ok(());
    };
    let decisions = super::apply_judgment(plan.decisions, &advice);
    let mut failures = ledger.counters.consecutive_failures;
    let opened = execute_plan(
        decisions,
        config.overseer.failure_circuit_threshold,
        &mut failures,
        |candidate| spawn_candidate(config, ledger, candidate, now),
        log_candidate,
    )?;
    ledger.counters.consecutive_failures = failures;
    if opened {
        open_circuit(config)?;
    }
    Ok(())
}

fn worker_modes() -> Result<HashMap<String, crate::model::ManagementMode>> {
    Ok(Registry::load()?
        .repos
        .into_iter()
        .flat_map(|repo| repo.agents)
        .map(|agent| (agent.id, agent.management))
        .collect())
}

fn execute_plan<F, L>(
    decisions: Vec<super::GateDecision>,
    threshold: u32,
    failures: &mut u32,
    mut spawn: F,
    mut log: L,
) -> Result<bool>
where
    F: FnMut(&Candidate) -> Result<SpawnOutcome>,
    L: FnMut(DecisionKind, &Candidate, &str) -> Result<()>,
{
    for decision in decisions {
        let Some(candidate) = decision.candidate else {
            continue;
        };
        if !decision.dispatch {
            log(DecisionKind::Skip, &candidate, &decision.reason)?;
            continue;
        }
        if *failures >= threshold {
            return Ok(true);
        }
        match spawn(&candidate) {
            Ok(SpawnOutcome::Spawned) => {}
            // A candidate the pre-spawn re-check held was never attempted, so it
            // leaves the failure budget for genuine spawn faults untouched.
            Ok(SpawnOutcome::Held(reason)) => log(DecisionKind::Hold, &candidate, &reason)?,
            Err(error) => {
                *failures = failures.saturating_add(1);
                log(
                    DecisionKind::Hold,
                    &candidate,
                    &format!("spawn_failed:{error}"),
                )?;
                if *failures >= threshold {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

fn open_circuit(config: &mut Config) -> Result<()> {
    config.overseer.dispatch_enabled = false;
    config.save()?;
    log_global(DecisionKind::CircuitOpen, "failure threshold reached")?;
    log_global(
        DecisionKind::Escalate,
        "dispatch disabled pending operator reset",
    )?;
    eprintln!("overseer: dispatch circuit opened; operator action required");
    Ok(())
}

fn gather_candidates() -> Result<Vec<Candidate>> {
    let registry = Registry::load()?;
    let (workspaces, overlay_ok) = dropr::DroprOverlay::load_with_status_timeout(COMMAND_TIMEOUT);
    if !overlay_ok {
        // Without the workspace overlay every repo would be skipped silently;
        // record the outage so an idle overseer is diagnosable from decisions.jsonl.
        log_global(DecisionKind::Skip, "dropr_overlay_unavailable")?;
        return Ok(Vec::new());
    }
    let mut candidates = Vec::new();
    for repo in &registry.repos {
        let Some(remote) = &repo.remote_url else {
            continue;
        };
        let Some(workspace) = workspaces.find_by_repo_url(remote) else {
            log_repo_skip(
                &repo.path.to_string_lossy(),
                "workspace_unmatched",
                logging::append,
            )?;
            continue;
        };
        if !repo.path.exists() {
            // Dispatching into a missing checkout fails the spawn and feeds
            // the failure circuit; skip stale registry entries instead.
            log_repo_skip(
                &repo.path.to_string_lossy(),
                "repo_path_missing",
                logging::append,
            )?;
            continue;
        }
        let tasks = match dropr::fetch_ready_dispatch_tasks_timeout(
            &workspace.id,
            READY_FETCH_LIMIT,
            COMMAND_TIMEOUT,
        ) {
            Ok(tasks) => tasks,
            Err(error) => {
                log_ready_failure(
                    &repo.path.to_string_lossy(),
                    &workspace.id,
                    error,
                    logging::append,
                )?;
                continue;
            }
        };
        for task in tasks {
            candidates.push(Candidate {
                task_id: if task.id.is_empty() {
                    task.task.display_id.clone()
                } else {
                    task.id
                },
                display_id: task.task.display_id,
                title: task.task.title,
                repo: repo.path.to_string_lossy().into_owned(),
                author: task.author,
                workspace: workspace.id.clone(),
            });
        }
    }
    Ok(candidates)
}

pub(super) fn log_candidate(kind: DecisionKind, task: &Candidate, reason: &str) -> Result<()> {
    let mut entry = DecisionEntry::new(kind, reason);
    entry.task = Some(task.task_id.clone());
    entry.repo = Some(task.repo.clone());
    entry.source = Some("dispatch".into());
    logging::append(&entry)
}

fn log_repo_skip<F>(repo: &str, reason: &str, append: F) -> Result<()>
where
    F: FnOnce(&DecisionEntry) -> Result<()>,
{
    let mut entry = DecisionEntry::new(DecisionKind::Skip, reason);
    entry.repo = Some(repo.into());
    entry.source = Some("dispatch".into());
    append(&entry)
}

fn log_ready_failure<F>(
    repo: &str,
    workspace: &str,
    error: dropr::ReadyDispatchError,
    append: F,
) -> Result<()>
where
    F: FnOnce(&DecisionEntry) -> Result<()>,
{
    let mut entry = DecisionEntry::new(DecisionKind::Skip, error.reason());
    entry.repo = Some(repo.into());
    entry.source = Some("dispatch".into());
    entry.reason = format!("{}:{workspace}", error.reason());
    append(&entry)
}

fn log_global(kind: DecisionKind, reason: &str) -> Result<()> {
    let mut entry = DecisionEntry::new(kind, reason);
    entry.source = Some("dispatch".into());
    logging::append(&entry)
}

#[cfg(test)]
#[path = "../judge/dispatch_runtime_tests.rs"]
mod tests;
