use super::ledger::{Ledger, LedgerEntry, LedgerPhase};
use chrono::{DateTime, Utc};

mod apply;
mod types;
use apply::{apply_inbox, apply_pr, apply_prerequisite_wait, apply_session, apply_task_failure};
pub use types::*;

#[rustfmt::skip]
pub fn reconcile(ledger: &Ledger, observations: &Observations, now: DateTime<Utc>, stuck_after_mins: u64, max_prerequisite_wait_hours: u64) -> (Ledger, Vec<Action>) {
    let mut next = ledger.clone();
    let mut actions = observation_errors(observations);
    drop_detached(&mut next, observations, &mut actions);
    for entry in &mut next.entries {
        if observations.manual_agents.contains(&entry.agent_id) {
            reconcile_manual_entry(entry, observations, now, &mut actions);
            continue;
        }
        reconcile_entry(entry, observations, now, stuck_after_mins, max_prerequisite_wait_hours, &mut actions);
    }
    (next, actions)
}
#[rustfmt::skip]
fn reconcile_entry(entry: &mut LedgerEntry, observations: &Observations, now: DateTime<Utc>, stuck_after_mins: u64, max_prerequisite_wait_hours: u64, actions: &mut Vec<Action>) {
    if entry.phase == LedgerPhase::Merged {
        if still_registered(entry, observations) {
            push_cleanup(entry, actions);
        }
        return;
    }
    let original = entry.phase;
    apply_inbox(entry, observations, actions);
    // Checked for every entry regardless of phase: the wait may have been set
    // here (a worker's `waiting-prerequisite` report) or by the auto-merge
    // pass finding the entry's pull request held on a `blocks` edge, and both
    // share this one bound. See `apply_prerequisite_wait`.
    apply_prerequisite_wait(entry, now, max_prerequisite_wait_hours, actions);
    apply_pr(entry, observations, actions);
    apply_task_failure(entry, observations, actions);
    // A worker waiting on a prerequisite is expected to have ended its turn —
    // the same way it does on `blocked` or `done` today — so its tmux session
    // going quiet is not the stuck-or-dead condition this check exists to
    // catch.
    if is_worker_phase(entry.phase) && entry.prerequisite_wait.is_none() {
        apply_session(entry, observations, now, stuck_after_mins, actions);
    }
    settle(entry, original, now);
    if original != LedgerPhase::Merged && entry.phase == LedgerPhase::Merged {
        push_cleanup(entry, actions);
    }
}
/// Record the instant an entry stopped being anyone's work.
///
/// Stamped from the pass's `now` rather than `Utc::now()` so the transition is
/// testable and every action of one pass shares one clock. Only on the
/// transition into a terminal phase: an entry that was already terminal when
/// the pass began keeps the timestamp it settled at, instead of having it
/// rewritten to the current time on every later poll. That also leaves entries
/// that settled before the field existed at `None` rather than back-dating them
/// to whenever the daemon was next restarted.
fn settle(entry: &mut LedgerEntry, original: LedgerPhase, now: DateTime<Utc>) {
    if !terminal(original) && terminal(entry.phase) {
        entry.settled_at = Some(now);
    }
}
/// Forget every entry whose worker is no longer an Overseer child.
///
/// Detaching a worker (`g` past Manual clears `parent_agent_id`) ends Overseer
/// ownership, but the registry row survives carrying `Manual`, so the entry used
/// to sit in the ledger frozen forever: never advanced, never cleaned up, and
/// still occupying a dispatch slot for a worker Overseer may no longer touch.
/// Dropping the row is what keeps the ledger and ownership from disagreeing —
/// a detached worktree is now exactly a hand-made one, which Overseer never
/// tracked in the first place.
///
/// The drop is unconditional on phase. Marking the entry terminal instead would
/// have to pick one of the existing terminal phases, and each one lies: `Failed`
/// reports a failure to dropr that never happened, and `Merged` runs the cleanup
/// that kills the session and removes the worktree — the exact opposite of a
/// detach, which leaves the worker running. The operator owns the worktree from
/// here; `robco` still kills it on request.
fn drop_detached(next: &mut Ledger, observations: &Observations, actions: &mut Vec<Action>) {
    next.entries.retain(|entry| {
        if !observations.detached_agents.contains(&entry.agent_id) {
            return true;
        }
        actions.push(Action::LogDecision {
            task_id: Some(entry.task_id.clone()),
            message: format!(
                "{}: dropped ledger entry; worker detached from overseer management",
                entry.display_id
            ),
        });
        false
    });
}
/// A Manual agent is driven by a human, so Overseer must never intervene in its
/// run: the inbox escalation, the dropr-lock escalation, and the dead/stuck
/// session failure path are all suppressed for it, and no dispatch ever targets
/// its task.
///
/// Advancing the phase is not an intervention, though. Skipping reconciliation
/// wholesale froze the entry at whatever phase it last had, so a merged PR never
/// reached [`LedgerPhase::Merged`] and its worktree, branch, and ledger row
/// leaked forever. PR state is therefore applied here exactly as it is for an
/// Auto agent.
///
/// `Merged` is terminal: the branch has landed and the human's work on that
/// worktree is over, so cleanup runs from there like any other entry.
/// [`crate::overseer::exec::execute_actions`] kills the session before touching
/// the worktree and defers removal when the kill fails, which is what keeps the
/// worktree from being pulled out from under a live shell.
fn reconcile_manual_entry(
    entry: &mut LedgerEntry,
    observations: &Observations,
    now: DateTime<Utc>,
    actions: &mut Vec<Action>,
) {
    if entry.phase == LedgerPhase::Merged {
        if still_registered(entry, observations) {
            push_cleanup(entry, actions);
        }
        return;
    }
    let original = entry.phase;
    apply_pr(entry, observations, actions);
    settle(entry, original, now);
    if entry.phase == LedgerPhase::Merged {
        push_cleanup(entry, actions);
    }
}
fn still_registered(entry: &LedgerEntry, observations: &Observations) -> bool {
    observations
        .registered_agents
        .iter()
        .any(|agent_id| agent_id == &entry.agent_id)
}
fn push_cleanup(entry: &LedgerEntry, actions: &mut Vec<Action>) {
    actions.push(Action::KillSession {
        agent_id: entry.agent_id.clone(),
    });
    actions.push(Action::RemoveWorktree {
        agent_id: entry.agent_id.clone(),
    });
}
fn is_worker_phase(phase: LedgerPhase) -> bool {
    matches!(
        phase,
        LedgerPhase::Dispatched | LedgerPhase::Claimed | LedgerPhase::Working
    )
}
fn terminal(phase: LedgerPhase) -> bool {
    matches!(
        phase,
        LedgerPhase::Merged | LedgerPhase::Failed | LedgerPhase::Escalated
    )
}
fn observation_errors(observations: &Observations) -> Vec<Action> {
    observations
        .errors
        .iter()
        .map(|error| Action::LogDecision {
            task_id: error.task_id.clone(),
            message: match &error.repo {
                Some(repo) => format!("observation skipped in {repo}: {}", error.message),
                None => format!("observation skipped: {}", error.message),
            },
        })
        .collect()
}
#[cfg(test)]
#[path = "monitor_observation_tests.rs"]
mod observation_tests;
#[cfg(test)]
#[path = "monitor_pr_tests.rs"]
mod pr_tests;
#[cfg(test)]
#[path = "monitor_tests.rs"]
mod tests;
