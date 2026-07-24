use chrono::{DateTime, Utc};

use super::Candidate;
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
    Held(&'static str),
}

pub(super) fn spawn_candidate(
    config: &Config,
    ledger: &mut Ledger,
    task: &Candidate,
    now: DateTime<Utc>,
) -> Result<SpawnOutcome> {
    // The plan was gated against the ledger as it stood before the pass began.
    // Re-check the live ledger so a task two registered repositories surfaced as
    // separate candidates cannot spawn a second worker onto the branch the first
    // one just claimed.
    if super::has_active_worker(ledger, &task.task_id, &task.display_id) {
        return Ok(SpawnOutcome::Held("active_worker"));
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
    let prompt = worker_prompt(&task.display_id, &task.task_id, &task.title, &task.repo);
    let outcome = spawn::spawn_in_repo(
        &task.repo,
        &task.title,
        name_slug(task).as_deref(),
        Some(&prompt),
        Some(OVERSEER_AGENT_ID),
        &extra_args,
        &worker_config,
    )?;
    ledger.entries.push(LedgerEntry {
        task_id: task.task_id.clone(),
        display_id: task.display_id.clone(),
        repo: task.repo.clone(),
        agent_id: outcome.id,
        branch: outcome.branch,
        phase: LedgerPhase::Dispatched,
        dispatched_at: now,
        retries: attempts,
        pr_url: None,
        branch_updates: 0,
    });
    ledger.counters.dispatched_today = ledger.counters.dispatched_today.saturating_add(1);
    super::runtime::log_candidate(DecisionKind::Dispatch, task, "worker spawned")?;
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

/// tmux-safe session name for the worker, or `None` when the display id is
/// empty and there is nothing to name the session after.
fn name_slug(task: &Candidate) -> Option<String> {
    let display_id = task
        .display_id
        .trim()
        .trim_start_matches('#')
        .strip_prefix("task-")
        .unwrap_or_else(|| task.display_id.trim().trim_start_matches('#'));
    (!display_id.is_empty()).then(|| {
        format!(
            "task-{display_id}-{}",
            crate::tmux::sanitize_target_part(&task.title)
        )
    })
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
            retries,
            pr_url: None,
            branch_updates: 0,
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
            }],
            Utc::now(),
            &HashMap::new(),
        );
        assert_eq!(plan.decisions[0].reason, "max_retries");
        assert!(!plan.decisions[0].dispatch);
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
