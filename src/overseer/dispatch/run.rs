//! `!run <task>`: dispatches exactly one named task — the operator's own
//! pick, not the machine's. Split out of `dispatch` to keep that file under
//! this project's source file size limit — see dropr:465.
//!
//! dropr:476 retired the polling dispatcher (`gather_candidates`, ordering,
//! the scheduling gate) along with the ready feed's use as a queue the
//! machine drains on its own. `run_named` never needed the *scheduling* half
//! of that gate — daily limits, retries, author allowlists, and
//! per-repository slots rationed what the machine chose for itself, never
//! what an operator already decided by naming a task. What survives here is
//! resolving the bare task ref to a [`Candidate`] and handing it to
//! [`worker::spawn_candidate`], which still refuses on its own: a live
//! worker or an existing branch for the task holds it exactly as before.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};

use super::{resolve, worker};
use crate::overseer::ledger::Ledger;
use crate::{Result, config::Config};

/// What `!run <task>` produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RunOutcome {
    Dispatched(String),
    Refused(String),
}

pub(crate) fn run_named(
    config: &Config,
    ledger: &mut Ledger,
    task: &str,
    now: DateTime<Utc>,
    unmaterialised_logged: &mut BTreeSet<String>,
    global_hold_logged: &mut Option<String>,
) -> Result<RunOutcome> {
    let candidate = match resolve::resolve_task(task, unmaterialised_logged, global_hold_logged)? {
        resolve::Resolved::OverlayUnavailable => {
            return Ok(RunOutcome::Refused("dropr_overlay_unavailable".into()));
        }
        resolve::Resolved::NotFound => return Ok(RunOutcome::Refused("not_ready".into())),
        resolve::Resolved::Found(candidate) => candidate,
    };
    match worker::spawn_candidate(config, ledger, &candidate, now, "operator_run")? {
        worker::SpawnOutcome::Spawned => Ok(RunOutcome::Dispatched(candidate.display_id.clone())),
        worker::SpawnOutcome::Held(reason) | worker::SpawnOutcome::Escalated(reason) => {
            Ok(RunOutcome::Refused(reason))
        }
    }
}
