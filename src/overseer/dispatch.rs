use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::model::ManagementMode;

use super::{config::OverseerConfig, judge::DispatchAdvice, ledger::Ledger};
use entries::{has_active_worker, task_entries, terminal, worker_mode};
use order::order_candidates;

mod claim;
mod entries;
pub(crate) mod naming;
mod order;
mod route;
mod runtime;
mod worker;
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

/// Applies advice only to candidates already approved by the deterministic gate.
pub(crate) fn apply_judgment(
    decisions: Vec<GateDecision>,
    advice: &DispatchAdvice,
) -> Vec<GateDecision> {
    let mut rejected = Vec::new();
    let mut approved = HashMap::new();
    let mut approved_order = Vec::new();
    for decision in decisions {
        if decision.dispatch {
            let id = decision
                .candidate
                .as_ref()
                .expect("approved candidate")
                .task_id
                .clone();
            approved_order.push(id.clone());
            approved.insert(id, decision);
        } else {
            rejected.push(decision);
        }
    }
    let mut advised = Vec::new();
    for id in &advice.candidate_ids {
        if let Some(decision) = approved.remove(id) {
            advised.push(decision);
        }
    }
    for id in approved_order {
        if let Some(mut decision) = approved.remove(&id) {
            decision.dispatch = false;
            decision.reason = format!("judge_filtered:{}", advice.reason);
            advised.push(decision);
        }
    }
    rejected.extend(advised);
    rejected
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
    // A limit of 0 means unlimited: dispatch is capped only by max_workers and
    // per_repo_limit. Guarding the comparison keeps `0` from reading as "already
    // at limit" (`0 >= 0`), which would silently skip every tick.
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

fn apply_candidate_gates(
    config: &OverseerConfig,
    ledger: &Ledger,
    candidates: &[Candidate],
    worker_modes: &HashMap<String, ManagementMode>,
    plan: &mut DispatchPlan,
) {
    // Every live worker is counted, Auto or Manual: `Ledger::active_workers` is
    // the one accounting both this gate and `robco overseer status` read, so the
    // cap enforced here is the count the operator sees.
    let active = ledger.active_workers();
    let mut global = active.count;
    let mut per_repo = active.repos;
    let mut selected_repos = HashSet::new();
    for candidate in candidates {
        let reason = candidate_skip(
            config,
            ledger,
            candidate,
            plan.dispatched_today
                .saturating_add(selected_repos.len() as u32),
            global,
            &per_repo,
            &selected_repos,
            worker_modes,
        );
        let dispatch = reason.is_none();
        if dispatch {
            global += 1;
            *per_repo.entry(candidate.repo.clone()).or_default() += 1;
            selected_repos.insert(candidate.repo.as_str());
        }
        plan.decisions.push(GateDecision {
            candidate: Some(candidate.clone()),
            dispatch,
            reason: reason.unwrap_or("ready").into(),
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn candidate_skip<'a>(
    config: &OverseerConfig,
    ledger: &Ledger,
    candidate: &Candidate,
    dispatched_today: u32,
    global: usize,
    per_repo: &BTreeMap<String, usize>,
    selected_repos: &HashSet<&str>,
    worker_modes: &HashMap<String, ManagementMode>,
) -> Option<&'a str> {
    // 0 = unlimited (see `plan_dispatch`); only enforce a positive cap.
    if config.daily_dispatch_limit != 0 && dispatched_today >= config.daily_dispatch_limit {
        return Some("daily_limit");
    }
    let recorded: Vec<_> =
        task_entries(ledger, &candidate.task_id, &candidate.display_id).collect();
    if recorded
        .iter()
        .any(|entry| worker_mode(entry, worker_modes) == ManagementMode::Manual)
    {
        return Some("manual");
    }
    // A worker in a non-terminal phase still owns this task's branch and worktree.
    // Dispatching a second worker onto it fails in `git worktree add` on the
    // existing branch, and those failures feed the circuit until dispatch latches
    // off — so hold the candidate for as long as its worker is alive, whatever
    // management mode owns it.
    if recorded.iter().any(|entry| !terminal(entry.phase)) {
        return Some("active_worker");
    }
    if ledger
        .skip_list
        .iter()
        .any(|id| id == &candidate.task_id || id == &candidate.display_id)
    {
        return Some("skip_list");
    }
    // `retries` counts the attempts already made against this task; `worker::
    // record_attempt` advances it on every attempt, including one whose spawn
    // failed before it could record an entry of its own.
    let retries = recorded
        .iter()
        .map(|entry| entry.retries)
        .max()
        .unwrap_or(0);
    if retries >= config.max_retries_per_task {
        return Some("max_retries");
    }
    if !config.dispatch_task_authors.is_empty()
        && !config.dispatch_task_authors.contains(&candidate.author)
    {
        return Some("author");
    }
    if global >= config.max_workers {
        return Some("max_workers");
    }
    if per_repo.get(candidate.repo.as_str()).copied().unwrap_or(0) >= config.per_repo_limit {
        return Some("per_repo_limit");
    }
    if selected_repos.contains(candidate.repo.as_str()) {
        return Some("one_per_repo");
    }
    None
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;
