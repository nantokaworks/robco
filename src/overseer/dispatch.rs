use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::model::ManagementMode;

use super::{config::OverseerConfig, ledger::Ledger};
use entries::{has_active_worker, task_entries};
use gate::apply_candidate_gates;
use order::order_candidates;

mod claim;
mod decision_log;
mod drain;
mod entries;
mod gate;
mod gather;
pub(crate) mod naming;
mod order;
mod run;
mod runtime;
mod worker;
pub(crate) use run::{RunOutcome, run_named};
pub use runtime::dispatch_pass;

/// Renders a daily dispatch limit for display, mapping the `0 = unlimited`
/// sentinel to `∞` so a zeroed-out cap never reads as a literal count.
pub fn format_dispatch_limit(limit: u32) -> String {
    if limit == 0 {
        "∞".to_string()
    } else {
        limit.to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Candidate {
    pub task_id: String,
    pub display_id: String,
    pub title: String,
    pub repo: String,
    pub author: String,
    /// dropr task priority (`high` / `medium` / `low`), verbatim. Empty when the
    /// ready feed omitted it. Feeds the deterministic ordering in
    /// [`order_candidates`], so a candidate whose priority is unknown sorts last
    /// rather than silently ahead of a stated one.
    pub priority: String,
    /// dropr workspace the task lives in. Carried from candidate gathering so
    /// the pre-spawn claim can address the task without re-resolving the
    /// repository's workspace.
    pub workspace: String,
    /// dropr's numeric refinement of `priority`, verbatim. `None` when the
    /// ready feed did not carry one. Feeds [`order_candidates`]'s intra-bucket
    /// tiebreak, refining `priority` rather than overriding it.
    pub priority_score: Option<i64>,
    /// dropr's task status (`open`, `in_progress`, `blocked`, ...), verbatim.
    /// `#[serde(default)]` so a request persisted by an older build still
    /// deserializes. `dispatch::gate` excludes `blocked` rather than trusting
    /// it already filtered upstream.
    #[serde(default)]
    pub status: String,
    /// This task's dropr `parent_task_id`, when it is a subtask. `dropr task
    /// ready` does not carry this itself, so `gather_candidates` batches a
    /// separate `dropr::fetch_parents` lookup per repo to fill it in.
    /// `None` covers both "this is a root task" and "the lookup could not
    /// tell" — `gate::candidate_skip`'s ancestor check treats both the same
    /// way, by not holding the candidate.
    #[serde(default)]
    pub parent_task_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateDecision {
    pub candidate: Option<Candidate>,
    pub dispatch: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchPlan {
    pub date: NaiveDate,
    pub dispatched_today: u32,
    pub dispatch_enabled: bool,
    pub circuit_opened: bool,
    pub decisions: Vec<GateDecision>,
}

pub fn plan_dispatch(
    config: &OverseerConfig,
    ledger: &Ledger,
    candidates: &[Candidate],
    now: DateTime<Utc>,
    worker_modes: &HashMap<String, ManagementMode>,
) -> DispatchPlan {
    let date = now.date_naive();
    let today = if ledger.counters.date == Some(date) {
        ledger.counters.dispatched_today
    } else {
        0
    };
    let mut plan = DispatchPlan {
        date,
        dispatched_today: today,
        dispatch_enabled: config.dispatch_enabled,
        circuit_opened: false,
        decisions: Vec::new(),
    };
    if !config.dispatch_enabled {
        // Distinguish an operator-intended disable from the failure circuit
        // latching dispatch off: once `open_circuit` persists
        // `dispatch_enabled = false`, this gate short-circuits every later tick
        // and would otherwise report the generic `dispatch_disabled`, hiding why
        // dispatch never resumes. Surfacing `circuit_open` keeps the latched
        // state legible in decisions.jsonl and the panel until the operator
        // resets it with `robco overseer set dispatch on`.
        let reason = if ledger.counters.consecutive_failures >= config.failure_circuit_threshold {
            "circuit_open"
        } else {
            "dispatch_disabled"
        };
        return global_skip(plan, reason);
    }
    // A limit of 0 means unlimited: dispatch is capped only by the per-repository
    // primary/secondary slots `apply_candidate_gates` enforces. Guarding the
    // comparison keeps `0` from reading as "already at limit" (`0 >= 0`), which
    // would silently skip every tick.
    if config.daily_dispatch_limit != 0 && today >= config.daily_dispatch_limit {
        return global_skip(plan, "daily_limit");
    }
    if ledger.counters.consecutive_failures >= config.failure_circuit_threshold {
        plan.dispatch_enabled = false;
        plan.circuit_opened = true;
        return global_skip(plan, "circuit_open");
    }
    // Order before gating, not after: the gates spend capacity as they walk the
    // list, so the order decides which candidates get the remaining slots.
    apply_candidate_gates(
        config,
        ledger,
        &order_candidates(candidates),
        worker_modes,
        &mut plan,
    );
    plan
}

fn global_skip(mut plan: DispatchPlan, reason: &str) -> DispatchPlan {
    plan.decisions.push(GateDecision {
        candidate: None,
        dispatch: false,
        reason: reason.into(),
    });
    plan
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;
