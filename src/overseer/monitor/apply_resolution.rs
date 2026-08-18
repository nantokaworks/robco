//! Reversing an escalation the daemon has evidence a human already answered.
//!
//! Split out of `apply.rs` on its own so the per-source `apply_*` functions
//! and this cross-source resolution pass stay separately readable.

use chrono::{DateTime, Utc};

use super::{Action, LedgerEntry, LedgerPhase, Observations};

/// `LedgerPhase::Escalated` used to be terminal in every practical sense: the
/// Inbox row it produces (`ui::inbox::aggregate`) reads the phase directly and
/// never re-checks it, so nothing but a human action ever moved an entry back
/// out. This looks for activity that happened strictly after
/// `entry.settled_at`, the instant the entry escalated, and treats any one
/// signal as enough:
///
/// - the worker's tmux session produced new output (`observations.sessions`),
/// - a new commit landed on the task branch (`observations.branches`), or
/// - dropr recorded a change to the task itself (`observations.tasks`).
///
/// `entry.settled_at` is the guard against pre-escalation activity: a signal
/// timestamped at or before it proves nothing, since it could just as well
/// predate the block. No baseline (`settled_at: None`, an old ledger row from
/// before the field existed) means nothing to compare against, so the entry
/// is left for a human exactly as it always was.
///
/// Also gated on `entry.worker_escalated`. `LedgerPhase::Escalated` is shared
/// with the merge subsystem's own safety-valve escalations — an
/// autonomy-envelope hard stop, an exhausted branch-update or recovery
/// budget, a pull request closed without merging — whose worktree and tmux session stay
/// alive exactly like a worker-blocked entry's do (`monitor::reconcile_entry`
/// keeps cleanup pending until `Merged`), so the same three signals fire for
/// them too. None of the three says anything about whether the specific
/// condition the merge gate raised has cleared, so only an entry whose
/// escalation actually came from a worker report is eligible here — see
/// `LedgerEntry::worker_escalated`.
pub(in crate::overseer::monitor) fn apply_escalation_resolution(
    entry: &mut LedgerEntry,
    observations: &Observations,
    actions: &mut Vec<Action>,
) {
    if entry.phase != LedgerPhase::Escalated || !entry.worker_escalated {
        return;
    }
    let Some(settled_at) = entry.settled_at else {
        return;
    };
    let after_settled = |at: DateTime<Utc>| at > settled_at;
    let signal = observations
        .sessions
        .iter()
        .find(|session| session.agent_id == entry.agent_id)
        .and_then(|session| session.last_activity_at)
        .filter(|at| after_settled(*at))
        .map(|_| "tmux_activity")
        .or_else(|| {
            observations
                .branches
                .iter()
                .find(|branch| branch.task_id == entry.task_id)
                .and_then(|branch| branch.latest_commit_at)
                .filter(|at| after_settled(*at))
                .map(|_| "new_commit")
        })
        .or_else(|| {
            observations
                .tasks
                .iter()
                .find(|task| task.task_id == entry.task_id)
                .and_then(|task| task.updated_at)
                .filter(|at| after_settled(*at))
                .map(|_| "dropr_task_updated")
        });
    if let Some(signal) = signal {
        resolve(entry, signal, actions);
    }
}

/// Common tail of both resolution paths — the explicit `unblocked` report in
/// `apply::apply_inbox` and the activity signals in
/// [`apply_escalation_resolution`] above. Both call sites already require
/// `entry.worker_escalated`, so this never fires for a merge-subsystem
/// escalation — but it clears the merge-hold reconsideration markers
/// regardless, the same way `daemon::merge_hold_recheck::settle` does,
/// because leaving them behind on an entry that goes on to earn a *different*
/// escalation later would misattribute that budget to a condition this
/// resolution has nothing to do with.
///
/// Lands on `Working` rather than whatever phase preceded the escalation,
/// which the ledger does not record: the next `apply_pr` / `apply_task_failure`
/// / `apply_session` in this same pass (or the next one, for a signal-driven
/// resolution that runs after them) re-derives the entry's real phase from
/// live state, the same way a freshly dispatched entry always has. Clearing
/// `settled_at` gives a later escalation of the same entry its own fresh
/// clock — `monitor::settle` only stamps a new one when the field reads
/// `None` — so a worker that reports blocked again is judged against its own
/// timestamp, not a stale one this resolution already spent.
pub(super) fn resolve(entry: &mut LedgerEntry, signal: &str, actions: &mut Vec<Action>) {
    entry.phase = LedgerPhase::Working;
    entry.settled_at = None;
    entry.worker_escalated = false;
    entry.merge_hold_cap_escalated = false;
    entry.merge_hold_rechecks = 0;
    entry.merge_hold_recheck_reason = None;
    entry.merge_hold_recheck_head = None;
    entry.merge_hold_stuck_notified = false;
    let reason = format!("resolved_externally:{signal}");
    actions.push(Action::Notify {
        message: format!("{}: {reason}", entry.display_id),
    });
    actions.push(Action::LogDecision {
        task_id: Some(entry.task_id.clone()),
        message: reason,
    });
}

#[cfg(test)]
#[path = "apply_resolution_tests.rs"]
mod tests;
