use chrono::{DateTime, Utc};

use super::{Candidate, claim, naming, naming::TaskSource};
use crate::overseer::{
    OVERSEER_AGENT_ID,
    ledger::{Ledger, LedgerEntry, LedgerPhase},
    logging::DecisionKind,
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
                let _ =
                    super::runtime::log_candidate(DecisionKind::Hold, task, "claim_release_failed");
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
    });
    ledger.counters.dispatched_today = ledger.counters.dispatched_today.saturating_add(1);
    super::runtime::log_candidate(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overseer::config::OverseerConfig;
    use crate::overseer::dispatch::plan_dispatch;
    use std::collections::HashMap;

    fn entry(task_id: &str, retries: u32) -> LedgerEntry {
        LedgerEntry {
            task_id: task_id.into(),
            display_id: "#1".into(),
            repo: "/repo".into(),
            agent_id: "auto-agent".into(),
            branch: "branch".into(),
            phase: LedgerPhase::Failed,
            dispatched_at: Utc::now(),
            settled_at: None,
            retries,
            pr_url: None,
            branch_updates: 0,
            merge_recovery: Default::default(),
            merge_hold: Default::default(),
            manual_merge_skip: None,
            merge_judge_fail_safes: 0,
        }
    }

    #[test]
    fn a_failed_spawn_still_counts_against_max_retries() {
        // The attempt is recorded before the spawn runs, so an attempt that never
        // reaches a ledger entry of its own still bounds the next pass.
        let mut ledger = Ledger::default();
        ledger.entries.push(entry("task-1", 0));

        assert_eq!(record_attempt(&mut ledger, "task-1", "#1"), 1);
        assert_eq!(ledger.entries[0].retries, 1);

        let plan = plan_dispatch(
            &OverseerConfig::default(),
            &ledger,
            &[Candidate {
                task_id: "task-1".into(),
                display_id: "#1".into(),
                title: "task".into(),
                repo: "/repo".into(),
                author: "allowed".into(),
                priority: "medium".into(),
                workspace: "workspace-1".into(),
                priority_score: None,
            }],
            Utc::now(),
            &HashMap::new(),
        );
        assert_eq!(plan.decisions[0].reason, "max_retries");
        assert!(!plan.decisions[0].dispatch);
    }

    fn candidate(display_id: &str, title: &str) -> Candidate {
        Candidate {
            task_id: "23O9SkXUps3lOIHuXvj4Z".into(),
            display_id: display_id.into(),
            title: title.into(),
            repo: "/repo".into(),
            author: "allowed".into(),
            priority: "medium".into(),
            workspace: "workspace-1".into(),
            priority_score: None,
        }
    }

    #[test]
    fn a_candidate_is_named_from_its_dropr_number() {
        // Shape coverage lives with the slug itself in `naming`; what matters
        // here is that a dispatched candidate is numbered from dropr.
        assert_eq!(
            name_slug(&candidate("#295", "Add a top-level language config")).as_deref(),
            Some("dropr-295-Add-a-top-level-language-config")
        );
        assert_eq!(name_slug(&candidate("", "Add a config")), None);
    }

    #[test]
    fn attempts_are_counted_across_both_identifiers() {
        let mut ledger = Ledger::default();
        ledger.entries.push(entry("task-1", 0));
        // An entry recorded under the display id belongs to the same task.
        ledger.entries.push(entry("#1", 0));

        assert_eq!(record_attempt(&mut ledger, "task-1", "#1"), 2);
        assert!(ledger.entries.iter().all(|entry| entry.retries == 2));
    }
}
