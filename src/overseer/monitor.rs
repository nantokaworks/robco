use super::ledger::{Ledger, LedgerEntry, LedgerPhase};
use chrono::{DateTime, Utc};

mod apply;
mod types;
use apply::{
    apply_escalation_resolution, apply_inbox, apply_pr, apply_prerequisite_wait, apply_session,
    apply_task_failure,
};
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
    // Last word for the pass: an entry every apply_* above left sitting at
    // `Escalated` (that is, `apply_inbox`'s `unblocked` report did not already
    // lift it) gets one more check against activity the daemon observed on
    // its own. A resolution here is picked back up by the normal pipeline on
    // the next pass, not this one — see `apply::apply_escalation_resolution`.
    apply_escalation_resolution(entry, observations, actions);
    settle(entry, now);
    if original != LedgerPhase::Merged && entry.phase == LedgerPhase::Merged {
        push_cleanup(entry, actions);
    }
}
/// Record the instant an entry stopped being anyone's work.
///
/// Stamped from the pass's `now` rather than `Utc::now()` so the transition is
/// testable and every action of one pass shares one clock. Driven off
/// `entry.settled_at` itself — terminal and unset gets stamped, terminal and
/// already stamped is left alone — rather than a phase comparison against the
/// start of the pass. That is what gives a re-escalation its own fresh clock
/// even when it happens in the very same pass a resolution cleared the field:
/// `apply::apply_escalation_resolution` and the `unblocked` report both clear
/// `settled_at` when they lift an escalation, so an entry `apply_task_failure`
/// re-escalates later in that same pass (finding the dropr task still
/// released, say) still reads as freshly unsettled here — the guarantee a
/// worker that reports blocked again after an auto-resolve depends on. An
/// entry that was already terminal when the pass began, and stays terminal,
/// keeps the timestamp it settled at rather than having it rewritten on every
/// later poll.
fn settle(entry: &mut LedgerEntry, now: DateTime<Utc>) {
    if terminal(entry.phase) && entry.settled_at.is_none() {
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
    apply_pr(entry, observations, actions);
    settle(entry, now);
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
#[path = "monitor_resolution_tests.rs"]
mod resolution_tests;
#[cfg(test)]
#[path = "monitor_tests.rs"]
mod tests;
