use chrono::{DateTime, Utc};

use super::{Candidate, claim, naming, naming::TaskSource};
use crate::overseer::{
    OVERSEER_AGENT_ID,
    exec::COMMAND_TIMEOUT,
    ledger::{Ledger, LedgerEntry, LedgerPhase},
    logging::DecisionKind,
    other_prs::OtherPrs,
    templates::worker_prompt,
};
use crate::{Result, config::Config, dropr, spawn};

/// What an approved candidate produced. A held candidate is not a spawn fault
/// and must not consume the failure budget the circuit reserves for genuine
/// spawn errors.
pub(super) enum SpawnOutcome {
    Spawned,
    Held(String),
    /// A candidate dispatch could never proceed on its own — e.g.
    /// `branch_exists` after `MAX_BRANCH_EXISTS_HOLDS` consecutive passes.
    /// Logged as `DecisionKind::Escalate` rather than `Hold`, so it reaches
    /// the alert digest and Inbox instead of being retried forever. Same
    /// failure-budget treatment as `Held`: not a spawn fault.
    Escalated(String),
}

/// Consecutive dispatch passes one candidate may be held on `branch_exists`
/// before it escalates. A left-over branch is not a transient state — only
/// an operator removing it, or the worker that owns it finishing, clears it
/// — so retrying every poll for days, as `dropr:ZJd6VtMdhDsD39-oeoq_L`'s
/// evidence shows happened, is wasted work that hides the real problem
/// instead of surfacing it.
const MAX_BRANCH_EXISTS_HOLDS: u32 = 5;

/// `route` names how this pass chose its dispatch set — `"auto"` for the
/// polled pass, `"operator_run"` for `!run` — and is written into the spawn's
/// decision entry so the log distinguishes the two.
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
        return Ok(branch_exists_outcome(ledger, &task.task_id, &branch));
    }
    // The branch conflict cleared — either the operator removed the
    // left-over branch, or this candidate never held one — so a later
    // conflict on this task starts its escalation count over rather than
    // inheriting one from a branch that is already gone.
    ledger.branch_exists_holds.remove(&task.task_id);
    // The ledger only knows about workers this overseer started. Re-read the
    // task in dropr and take its claim here, so an agent that claimed it since
    // this pass started gathering candidates is seen now rather than
    // discovered when two workers are already sharing a branch.
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
    // Read next to where the display id is captured, the way defect 1
    // (dropr:yD5Gf6TX23VMvuSLFsmvO) asks: a leaf task simply comes back
    // empty, which renders the same childless prompt as before.
    let subtasks = dropr::fetch_subtasks(&task.workspace, &task.task_id, COMMAND_TIMEOUT);
    let prompt = worker_prompt(
        &task.display_id,
        &task.task_id,
        &task.title,
        &task.repo,
        &subtasks,
        config.language.as_deref(),
        config.overseer.worker_prompt_template.as_deref(),
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
        merge_hold_cap_escalated: false,
        merge_hold_rechecks: 0,
        merge_hold_recheck_reason: None,
        merge_hold_recheck_head: None,
        prerequisite_wait: None,
        merge_hold_stuck_notified: false,
        escalation_notified_reason: None,
        escalation_notified_head: None,
        worker_escalated: false,
        operator_override: None,
        merge_approval: None,
        pr_facts: None,
    });
    ledger.counters.dispatched_today = ledger.counters.dispatched_today.saturating_add(1);
    super::decision_log::log_candidate(
        DecisionKind::Dispatch,
        task,
        &format!("worker spawned:{route}"),
    )?;
    Ok(SpawnOutcome::Spawned)
}

/// Charges one `branch_exists` hold against `task_id` and decides whether the
/// candidate should keep being held or escalate.
///
/// `Escalated` fires exactly once, on the pass that spends the bound — the
/// count is left at `MAX_BRANCH_EXISTS_HOLDS` rather than growing further, so
/// every later pass while the branch is still there returns the steady
/// `branch_exists_escalated` hold instead of re-escalating on every poll.
fn branch_exists_outcome(ledger: &mut Ledger, task_id: &str, branch: &str) -> SpawnOutcome {
    let holds = ledger
        .branch_exists_holds
        .entry(task_id.to_owned())
        .or_insert(0);
    if *holds >= MAX_BRANCH_EXISTS_HOLDS {
        return SpawnOutcome::Held(format!("branch_exists_escalated:{branch}"));
    }
    *holds = holds.saturating_add(1);
    if *holds >= MAX_BRANCH_EXISTS_HOLDS {
        return SpawnOutcome::Escalated(format!("branch_exists:{branch}"));
    }
    SpawnOutcome::Held(format!("branch_exists:{branch}"))
}

/// Counts this launch attempt against every ledger entry already tracking the
/// task and returns the number of attempts that preceded it.
///
/// The count is recorded before the spawn is tried: a spawn that fails writes no
/// entry of its own, so leaving `retries` frozen at its spawn-time value would
/// keep the attempt invisible to anyone reading the ledger's history for this
/// task.
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
