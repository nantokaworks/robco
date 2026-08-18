use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::mem;

use super::{
    Candidate,
    decision_log::{log_candidate_once, log_global, log_global_once},
    drain,
    gather::gather_candidates,
    plan_dispatch,
    route::{Route, remaining_capacity, route},
    worker::{SpawnOutcome, spawn_candidate},
};
use crate::overseer::{
    config_write,
    judge::JudgmentQueue,
    ledger::Ledger,
    logging::{self, DecisionKind},
};
use crate::{Result, config::Config, registry::Registry};

pub fn dispatch_pass(
    config: &mut Config,
    ledger: &mut Ledger,
    now: DateTime<Utc>,
    judgments: &mut JudgmentQueue,
    unmaterialised_logged: &mut BTreeSet<String>,
    dispatch_hold_logged: &mut BTreeMap<String, String>,
    dispatch_global_hold_logged: &mut Option<String>,
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
        log_global_once(
            DecisionKind::Skip,
            &decision.reason,
            dispatch_global_hold_logged,
            logging::append,
        )?;
        return Ok(());
    }

    let Some(candidates) = gather_candidates(unmaterialised_logged, dispatch_global_hold_logged)?
    else {
        // The gather itself failed (e.g. `dropr_overlay_unavailable`, already
        // logged inside `gather_candidates`); an empty result here is not
        // evidence the board is quiet, so the queue-drained check must not
        // run on it.
        return Ok(());
    };
    // Every global gate cleared this pass, so whatever blocked an earlier
    // pass — this reason or a different one — is free to log fresh if it
    // recurs later.
    *dispatch_global_hold_logged = None;
    // A task id that left the candidate list (dispatched, closed, or no
    // longer offered) leaves no stale entry behind to wrongly suppress a
    // later, genuinely new decision about the same task id.
    let candidate_ids: BTreeSet<&str> = candidates.iter().map(|c| c.task_id.as_str()).collect();
    dispatch_hold_logged.retain(|task_id, _| candidate_ids.contains(task_id.as_str()));
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
    let capacity = remaining_capacity(&config.overseer, ledger, &approved);
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
        |kind, candidate, reason| {
            log_candidate_once(
                kind,
                candidate,
                reason,
                dispatch_hold_logged,
                logging::append,
            )
        },
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
            // Same failure-budget treatment as `Held` — an escalated candidate
            // never reached a spawn attempt either — logged as `Escalate` so it
            // reaches the alert digest instead of reading as a routine hold.
            Ok(SpawnOutcome::Escalated(reason)) => {
                log(DecisionKind::Escalate, &candidate, &reason)?
            }
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

#[cfg(test)]
#[path = "../judge/dispatch_runtime_tests.rs"]
mod tests;
