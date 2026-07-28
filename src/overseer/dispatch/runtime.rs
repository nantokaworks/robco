use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, HashMap};
use std::mem;

use super::{
    Candidate, drain, plan_dispatch,
    route::{Route, remaining_capacity, route},
    worker::{SpawnOutcome, spawn_candidate},
};
use crate::overseer::{
    config_write,
    exec::COMMAND_TIMEOUT,
    judge::JudgmentQueue,
    ledger::Ledger,
    logging::{self, DecisionEntry, DecisionKind},
};
use crate::{Result, config::Config, dropr, dropr::READY_FETCH_LIMIT, registry::Registry};

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

    let Some(candidates) = gather_candidates()? else {
        // The gather itself failed (e.g. `dropr_overlay_unavailable`, already
        // logged inside `gather_candidates`); an empty result here is not
        // evidence the board is quiet, so the queue-drained check must not
        // run on it.
        return Ok(());
    };
    drain::check(&candidates, ledger)?;
    let plan = plan_dispatch(&config.overseer, ledger, &candidates, now, &worker_modes);
    let approved = plan
        .decisions
        .iter()
        .filter(|decision| decision.dispatch)
        .filter_map(|decision| decision.candidate.clone())
        .collect::<Vec<_>>();
    // Before routing, and on every pass: a round the candidate set outran is
    // dead whether or not this pass wants a judge, and it must not vanish
    // unrecorded.
    judgments.discard_stale_dispatch(&approved)?;
    let capacity = remaining_capacity(&config.overseer, ledger, &approved, &worker_modes);
    let taken = route(
        approved.len(),
        capacity,
        config.overseer.judge_profile.is_some(),
    );
    let decisions = match taken {
        Route::Direct(_) => plan.decisions,
        Route::Judged => {
            let Some(advice) = judgments.dispatch_advice(&approved) else {
                return Ok(());
            };
            super::apply_judgment(plan.decisions, &advice)
        }
    };
    // Taken out of the ledger rather than borrowed from it: `spawn` below needs
    // `ledger` mutably too, and the two budgets are unrelated (see
    // `Ledger::dispatch_failure_streaks`), so there is nothing to reconcile by
    // keeping them aliased for the loop's duration.
    let mut streaks = mem::take(&mut ledger.dispatch_failure_streaks);
    let tripped = execute_plan(
        decisions,
        config.overseer.failure_circuit_threshold,
        &mut streaks,
        |candidate| spawn_candidate(config, ledger, candidate, now, taken.label()),
        log_candidate,
    )?;
    ledger.dispatch_failure_streaks = streaks;
    // A candidate that exhausted its own budget stops being redispatched from
    // here on, the same mechanism an operator uses manually — but dispatch
    // stays enabled for every other repository; see `execute_plan`.
    for candidate in tripped {
        if !ledger.skip_list.contains(&candidate.task_id) {
            ledger.skip_list.push(candidate.task_id);
        }
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

/// Runs every dispatchable decision, tracking spawn failures per candidate
/// (`streaks`, keyed by `Candidate::task_id`) rather than in one pass-wide
/// total. A candidate that reaches `threshold` consecutive failures is
/// returned in the result so the caller can take it out of rotation — see
/// `dispatch_pass` — instead of this function reaching for the global circuit
/// itself: one candidate's own budget must never gate every other repository's
/// dispatch (dropr:_ord_VtFSIiLgWpgmDAGm).
fn execute_plan<F, L>(
    decisions: Vec<super::GateDecision>,
    threshold: u32,
    streaks: &mut BTreeMap<String, u32>,
    mut spawn: F,
    mut log: L,
) -> Result<Vec<Candidate>>
where
    F: FnMut(&Candidate) -> Result<SpawnOutcome>,
    L: FnMut(DecisionKind, &Candidate, &str) -> Result<()>,
{
    let mut tripped = Vec::new();
    for decision in decisions {
        let Some(candidate) = decision.candidate else {
            continue;
        };
        if !decision.dispatch {
            log(DecisionKind::Skip, &candidate, &decision.reason)?;
            continue;
        }
        match spawn(&candidate) {
            Ok(SpawnOutcome::Spawned) => {
                streaks.remove(&candidate.task_id);
            }
            // A candidate the pre-spawn re-check held was never attempted, so it
            // leaves the failure budget for genuine spawn faults untouched.
            Ok(SpawnOutcome::Held(reason)) => log(DecisionKind::Hold, &candidate, &reason)?,
            Err(error) => {
                let streak = streaks.entry(candidate.task_id.clone()).or_insert(0);
                *streak = streak.saturating_add(1);
                log(
                    DecisionKind::Hold,
                    &candidate,
                    &format!("spawn_failed:{error}"),
                )?;
                if *streak >= threshold {
                    streaks.remove(&candidate.task_id);
                    log(
                        DecisionKind::CircuitOpen,
                        &candidate,
                        "candidate_circuit_open",
                    )?;
                    tripped.push(candidate);
                }
            }
        }
    }
    Ok(tripped)
}

fn open_circuit(config: &mut Config) -> Result<()> {
    config.overseer.dispatch_enabled = false;
    // The snapshot this pass carries can be minutes old by now, so persist the one
    // field the circuit owns rather than writing the whole stale struct back over
    // an operator's edits. The CircuitOpen entry below records the rewrite.
    config_write::persist_dispatch_enabled(false)?;
    log_global(DecisionKind::CircuitOpen, "failure threshold reached")?;
    log_global(
        DecisionKind::Escalate,
        "dispatch disabled pending operator reset",
    )?;
    eprintln!("overseer: dispatch circuit opened; operator action required");
    Ok(())
}

/// `None` means the gather itself failed (currently: the dropr workspace
/// overlay was unreachable) — distinct from `Some(vec![])`, a gather that
/// succeeded and simply found nothing ready. Callers that treat "no
/// candidates" as a board signal (the queue-drained check) must tell the two
/// apart, or an outage reads as "all done".
fn gather_candidates() -> Result<Option<Vec<Candidate>>> {
    let registry = Registry::load()?;
    let (workspaces, overlay_ok) = dropr::DroprOverlay::load_with_status_timeout(COMMAND_TIMEOUT);
    if !overlay_ok {
        // Without the workspace overlay every repo would be skipped silently;
        // record the outage so an idle overseer is diagnosable from decisions.jsonl.
        log_global(DecisionKind::Skip, "dropr_overlay_unavailable")?;
        return Ok(None);
    }
    let mut candidates = Vec::new();
    for repo in &registry.repos {
        if repo.management == crate::model::ManagementMode::Manual {
            // An operator working a repo by hand opted it out; recording this
            // as its own skip reason (rather than falling through to
            // `workspace_unmatched`) is what keeps an idle Overseer
            // diagnosable from `decisions.jsonl` alone.
            log_repo_skip(
                &repo.path.to_string_lossy(),
                "overseer_unmanaged",
                logging::append,
            )?;
            continue;
        }
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
                priority: task.task.priority,
                workspace: workspace.id.clone(),
                priority_score: task.task.priority_score,
            });
        }
    }
    Ok(Some(candidates))
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
