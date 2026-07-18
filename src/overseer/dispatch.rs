use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

use crate::model::ManagementMode;

use super::{
    config::OverseerConfig,
    judge::DispatchAdvice,
    ledger::{Ledger, LedgerPhase},
};

mod runtime;
pub use runtime::dispatch_pass;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Candidate {
    pub task_id: String,
    pub display_id: String,
    pub title: String,
    pub repo: String,
    pub author: String,
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
        return global_skip(plan, "dispatch_disabled");
    }
    if today >= config.daily_dispatch_limit {
        return global_skip(plan, "daily_limit");
    }
    if ledger.counters.consecutive_failures >= config.failure_circuit_threshold {
        plan.dispatch_enabled = false;
        plan.circuit_opened = true;
        return global_skip(plan, "circuit_open");
    }
    apply_candidate_gates(config, ledger, candidates, worker_modes, &mut plan);
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
    let active: Vec<_> = ledger
        .entries
        .iter()
        .filter(|entry| !terminal(entry.phase))
        .filter(|entry| worker_mode(entry, worker_modes) == ManagementMode::Auto)
        .collect();
    let mut global = active.len();
    let mut per_repo: HashMap<&str, usize> = HashMap::new();
    for entry in &active {
        *per_repo.entry(&entry.repo).or_default() += 1;
    }
    let mut selected_repos = HashSet::new();
    for candidate in candidates {
        let reason = candidate_skip(
            config,
            ledger,
            candidate,
            plan.dispatched_today + selected_repos.len() as u32,
            global,
            &per_repo,
            &selected_repos,
            worker_modes,
        );
        let dispatch = reason.is_none();
        if dispatch {
            global += 1;
            *per_repo.entry(&candidate.repo).or_default() += 1;
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
    per_repo: &HashMap<&str, usize>,
    selected_repos: &HashSet<&str>,
    worker_modes: &HashMap<String, ManagementMode>,
) -> Option<&'a str> {
    if dispatched_today >= config.daily_dispatch_limit {
        return Some("daily_limit");
    }
    let matching_workers: Vec<_> = ledger
        .entries
        .iter()
        .filter(|entry| {
            entry.task_id == candidate.task_id || entry.display_id == candidate.display_id
        })
        .collect();
    if matching_workers
        .iter()
        .any(|entry| worker_mode(entry, worker_modes) == ManagementMode::Manual)
    {
        return Some("manual");
    }
    if ledger
        .skip_list
        .iter()
        .any(|id| id == &candidate.task_id || id == &candidate.display_id)
    {
        return Some("skip_list");
    }
    let retries = ledger
        .entries
        .iter()
        .filter(|entry| entry.task_id == candidate.task_id)
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

fn worker_mode(
    entry: &super::ledger::LedgerEntry,
    worker_modes: &HashMap<String, ManagementMode>,
) -> ManagementMode {
    worker_modes
        .get(&entry.agent_id)
        .copied()
        .unwrap_or(ManagementMode::Auto)
}

fn terminal(phase: LedgerPhase) -> bool {
    matches!(
        phase,
        LedgerPhase::Merged | LedgerPhase::Failed | LedgerPhase::Escalated
    )
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;
