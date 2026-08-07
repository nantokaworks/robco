use chrono::{DateTime, Utc};

use super::{Candidate, claim, naming, naming::TaskSource};
use crate::overseer::{
    OVERSEER_AGENT_ID,
    ledger::{Ledger, LedgerEntry, LedgerPhase},
    logging::DecisionKind,
    other_prs::OtherPrs,
    templates::worker_prompt,
};
use crate::{Result, config::Config, spawn};

/// What an approved candidate produced. A held candidate is not a spawn fault
/// and must not consume the failure budget the circuit reserves for genuine
/// spawn errors.
pub(super) enum SpawnOutcome {
    Spawned,
    Held(String),
}

/// `route` names how this pass chose its dispatch set (see
/// `super::route::Route`). It is written into the spawn's decision entry so the
/// log distinguishes a dispatch an LLM judge approved from one the
/// deterministic gate made on its own.
pub(super) fn spawn_candidate(
    config: &Config,
    ledger: &mut Ledger,
    task: &Candidate,
    now: DateTime<Utc>,
    route: &str,
) -> Result<SpawnOutcome> {
    // The plan was gated against the ledger as it stood before the pass began.
    // Re-check the live ledger so a task two registered repositories surfaced as
    // separate candidates cannot spawn a second worker onto the branch the first
    // one just claimed.
    if super::has_active_worker(ledger, &task.task_id, &task.display_id) {
        return Ok(SpawnOutcome::Held("active_worker".into()));
    }
    // The ledger only knows what the overseer itself dispatched, so a task an
    // outside agent already finished never shows up as `active_worker` above.
    // This is the same blind spot dropr task #350 surfaces for display — here
    // it stops a worker from starting on work a pull request already covers,
    // whatever the dropr task's own status still says (dropr task #354).
    if let Some(reason) = closed_elsewhere(&OtherPrs::load()?, task) {
        return Ok(SpawnOutcome::Held(reason));
    }
    // A prior attempt at this same task can leave its branch behind — most
    // commonly the escalated case, where the worker finished and opened a pull
    // request but the branch itself was never cleaned up. `git worktree add`
    // is certain to fail on that branch name, and doing so on every pass is
    // exactly the loop this check exists to stop (dropr:_ord_VtFSIiLgWpgmDAGm).
    // Held rather than reused or reclaimed: the branch may still carry
    // uncommitted or unmerged work from that prior attempt, so the safe move is
    // to leave it alone and let an operator decide, not to guess.
    if let Some(branch) =
        spawn::branch_conflict(&task.repo, &task.title, name_slug(task).as_deref(), config)?
    {
        return Ok(SpawnOutcome::Held(format!("branch_exists:{branch}")));
    }
    // The ledger only knows about workers this overseer started. Re-read the
    // task in dropr and take its claim here, so an agent that claimed it while
    // the judge round was in flight is seen now rather than discovered when two
    // workers are already sharing a branch.
    if let claim::Acquired::Held(reason) = claim::acquire(task, OVERSEER_AGENT_ID, now)? {
        return Ok(SpawnOutcome::Held(reason));
    }
    let attempts = record_attempt(ledger, &task.task_id, &task.display_id);
    let mut worker_config = config.clone();
    if let Some(profile) = &config.overseer.worker_profile {
        worker_config.default_program.clone_from(profile);
    }
    let extra_args = worker_config
        .profiles
        .iter()
        .find(|profile| profile.name == worker_config.default_program)
        .map(|profile| profile.autonomous_args.clone())
        .unwrap_or_default();
    let prompt = worker_prompt(
        &task.display_id,
        &task.task_id,
        &task.title,
        &task.repo,
        config.language.as_deref(),
    );
    let outcome = match spawn::spawn_in_repo(
        &task.repo,
        &task.title,
        name_slug(task).as_deref(),
        Some(&prompt),
        Some(OVERSEER_AGENT_ID),
        &extra_args,
        &worker_config,
    ) {
        Ok(outcome) => outcome,
        Err(error) => {
            // The claim was taken for a worker that never started. Holding it
            // for the full TTL would lock the task away from the next pass and
            // from any operator, so hand it straight back — and say so when the
            // hand-back itself fails, since the task is then parked invisibly.
            if !claim::release(task, OVERSEER_AGENT_ID) {
                let _ = super::decision_log::log_candidate(
                    DecisionKind::Hold,
                    task,
                    "claim_release_failed",
                );
            }
            return Err(error);
        }
    };
    ledger.entries.push(LedgerEntry {
        task_id: task.task_id.clone(),
        display_id: task.display_id.clone(),
        repo: task.repo.clone(),
        agent_id: outcome.id,
        branch: outcome.branch,
        phase: LedgerPhase::Dispatched,
        dispatched_at: now,
        settled_at: None,
        retries: attempts,
        pr_url: None,
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        manual_merge_skip: None,
        merge_judge_fail_safes: 0,
        merge_hold_cap_escalated: false,
        merge_hold_rechecks: 0,
        merge_hold_recheck_reason: None,
        merge_hold_recheck_head: None,
        prerequisite_wait: None,
        merge_hold_stuck_notified: false,
        worker_escalated: false,
    });
    ledger.counters.dispatched_today = ledger.counters.dispatched_today.saturating_add(1);
    super::decision_log::log_candidate(
        DecisionKind::Dispatch,
        task,
        &format!("worker spawned:{route}"),
    )?;
    Ok(SpawnOutcome::Spawned)
}

/// Counts this dispatch attempt against every ledger entry already tracking the
/// task and returns the number of attempts that preceded it.
///
/// The count is recorded before the spawn is tried: a spawn that fails writes no
/// entry of its own, so leaving `retries` frozen at its spawn-time value would
/// keep the attempt invisible to `max_retries_per_task` and let the task be
/// re-dispatched every pass until the failure circuit latched dispatch off.
fn record_attempt(ledger: &mut Ledger, task_id: &str, display_id: &str) -> u32 {
    let attempts = super::task_entries(ledger, task_id, display_id).count() as u32;
    for entry in ledger
        .entries
        .iter_mut()
        .filter(|entry| entry.task_id == task_id || entry.display_id == display_id)
    {
        entry.retries = entry.retries.max(attempts);
    }
    attempts
}

/// Name for the worker's worktree, branch, and session. Candidates reach
/// dispatch through the dropr poller, so their numbers come from dropr's
/// display ids.
fn name_slug(task: &Candidate) -> Option<String> {
    naming::name_slug(TaskSource::Dropr, &task.display_id, &task.title)
}

/// A skip reason, named after the pull request, when `other_prs` already
/// carries one that closes `task` — separated from `spawn_candidate` so this
/// decision is tested against an in-memory fixture rather than the real
/// `~/.robco/overseer/other_prs.json`.
fn closed_elsewhere(other_prs: &OtherPrs, task: &Candidate) -> Option<String> {
    other_prs
        .closing(&task.repo, &task.display_id)
        .map(|pr| format!("pr_closes_task:{}", pr.url))
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
