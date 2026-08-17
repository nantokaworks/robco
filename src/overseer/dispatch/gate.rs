//! The per-candidate dispatch gate: whether a candidate proceeds, and the
//! exact reason when it does not. Split out of `dispatch` to keep that file
//! under this project's source file size limit.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::model::ManagementMode;
use crate::overseer::{config::OverseerConfig, ledger::Ledger};

use super::{
    Candidate, DispatchPlan, GateDecision,
    entries::{has_active_worker, holds_capacity, task_entries, worker_mode},
};

pub(super) fn apply_candidate_gates(
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
    // dropr's status already says the task cannot proceed; re-dispatching it
    // every pass just re-derives that. Self-clearing: `open` is the only exit.
    if candidate.status == "blocked" {
        return Some("blocked");
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
    // management mode owns it. An entry merely waiting on a dropr `blocks`
    // dependency edge is excluded (`holds_capacity`): its worker already
    // stepped aside, and dropr's own ready feed will not offer this task back
    // until the prerequisite closes anyway — see dropr:375.
    //
    // A non-terminal entry that already carries a `pr_url` is a different case
    // worth naming on its own: the worker is done and a pull request is open, so
    // re-dispatching would not just collide with a live worktree, it would retry
    // work that already shipped. The operator's move is on the pull request, not
    // on this task, and `pr_already_open` says so instead of reading like the
    // worker is still running.
    if let Some(entry) = recorded.iter().find(|entry| holds_capacity(entry)) {
        return Some(if entry.pr_url.is_some() {
            "pr_already_open"
        } else {
            "active_worker"
        });
    }
    // A RUN dispatch against the parent already covers this subtask's own
    // implementation (dropr:yD5Gf6TX23VMvuSLFsmvO): the parent's own ledger
    // entry is recorded under the parent's task/display id, not this
    // candidate's, so `recorded` above never sees it. Without this check a
    // free worker slot dispatches the subtask separately while the parent's
    // worker is still building the same change.
    if let Some(parent_task_id) = &candidate.parent_task_id
        && has_active_worker(ledger, parent_task_id, parent_task_id)
    {
        return Some("parent_worker_active");
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
