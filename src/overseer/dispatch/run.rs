//! `!run <task>`: dispatches exactly one named task through the same gate
//! and spawn path a polled candidate goes through. Split out of `dispatch`
//! to keep that file under this project's source file size limit — see
//! dropr:465.

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Utc};

use super::{
    Candidate, DispatchPlan, decision_log, gate::apply_candidate_gates, gather, runtime, worker,
};
use crate::model::ManagementMode;
use crate::overseer::{config::OverseerConfig, ledger::Ledger, logging::DecisionKind};
use crate::{Result, config::Config};

/// What `!run <task>` produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RunOutcome {
    Dispatched(String),
    Refused(String),
}

/// Dispatches exactly one named task through the same gate and spawn path a
/// polled candidate goes through — see dropr:465.
///
/// Named dispatch is a *selection*, not an override: this still gathers the
/// pass's whole ready feed the normal way (so the gate sees the same
/// candidate list a polled pass would) and filters it down to the one
/// candidate matching `task`, so every per-candidate check in
/// [`apply_candidate_gates`] still runs — including the daily dispatch
/// limit and the per-repository slot accounting, both of which read straight
/// off the live `ledger`, so a slot an ordinary `dispatch_pass` call already
/// spent earlier this same tick is still respected here.
///
/// The one thing this deliberately skips is [`super::plan_dispatch`]'s own
/// *global* pre-check — `dispatch_enabled` / `circuit_open` / the pass-wide
/// daily-limit short-circuit — because the task body requires `!run` to work
/// whether auto-dispatch is on or off. It chooses the candidate; it does not
/// overrule the gate that candidate still has to clear.
pub(crate) fn run_named(
    config: &Config,
    ledger: &mut Ledger,
    task: &str,
    now: DateTime<Utc>,
    unmaterialised_logged: &mut BTreeSet<String>,
    global_hold_logged: &mut Option<String>,
) -> Result<RunOutcome> {
    let Some(candidates) = gather::gather_candidates(unmaterialised_logged, global_hold_logged)?
    else {
        return Ok(RunOutcome::Refused("dropr_overlay_unavailable".into()));
    };
    let worker_modes = runtime::worker_modes()?;
    let candidate = match select_and_gate(
        &candidates,
        task,
        &config.overseer,
        ledger,
        now,
        &worker_modes,
    ) {
        Ok(candidate) => candidate,
        Err(outcome) => {
            if let RunOutcome::Refused(reason) = &outcome
                && let Some(candidate) = candidates
                    .iter()
                    .find(|candidate| candidate.task_id == task || candidate.display_id == task)
            {
                decision_log::log_candidate(DecisionKind::Skip, candidate, reason)?;
            }
            return Ok(outcome);
        }
    };
    match worker::spawn_candidate(config, ledger, candidate, now, "operator_run")? {
        worker::SpawnOutcome::Spawned => Ok(RunOutcome::Dispatched(candidate.display_id.clone())),
        worker::SpawnOutcome::Held(reason) | worker::SpawnOutcome::Escalated(reason) => {
            Ok(RunOutcome::Refused(reason))
        }
    }
}

/// The candidate-selection and per-candidate-gate half of [`run_named`],
/// split out so it is testable without the real dropr/registry I/O
/// `gather::gather_candidates` does. `Ok` means the candidate cleared the
/// gate and is ready for [`worker::spawn_candidate`]; `Err` carries the
/// refusal `run_named` should return as-is — `not_ready` when `task` is not
/// in this pass's ready feed at all, or the gate's own reason otherwise.
pub(super) fn select_and_gate<'a>(
    candidates: &'a [Candidate],
    task: &str,
    config: &OverseerConfig,
    ledger: &Ledger,
    now: DateTime<Utc>,
    worker_modes: &HashMap<String, ManagementMode>,
) -> std::result::Result<&'a Candidate, RunOutcome> {
    let Some(candidate) = candidates
        .iter()
        .find(|candidate| candidate.task_id == task || candidate.display_id == task)
    else {
        return Err(RunOutcome::Refused("not_ready".into()));
    };
    let date = now.date_naive();
    let mut plan = DispatchPlan {
        date,
        dispatched_today: if ledger.counters.date == Some(date) {
            ledger.counters.dispatched_today
        } else {
            0
        },
        dispatch_enabled: config.dispatch_enabled,
        circuit_opened: false,
        decisions: Vec::new(),
    };
    apply_candidate_gates(
        config,
        ledger,
        std::slice::from_ref(candidate),
        worker_modes,
        &mut plan,
    );
    let decision = plan
        .decisions
        .into_iter()
        .next()
        .expect("one candidate produces exactly one decision");
    if decision.dispatch {
        Ok(candidate)
    } else {
        Err(RunOutcome::Refused(decision.reason))
    }
}
