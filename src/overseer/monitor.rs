use super::ledger::{Ledger, LedgerEntry, LedgerPhase};
use chrono::{DateTime, Duration, Utc};

mod types;
pub use types::*;

#[rustfmt::skip]
pub fn reconcile(ledger: &Ledger, observations: &Observations, now: DateTime<Utc>, stuck_after_mins: u64) -> (Ledger, Vec<Action>) {
    let mut next = ledger.clone();
    let mut actions = observation_errors(observations);
    for entry in &mut next.entries {
        if observations.manual_agents.contains(&entry.agent_id) {
            reconcile_manual_entry(entry, observations, &mut actions);
            continue;
        }
        reconcile_entry(entry, observations, now, stuck_after_mins, &mut actions);
    }
    (next, actions)
}
#[rustfmt::skip]
fn reconcile_entry(entry: &mut LedgerEntry, observations: &Observations, now: DateTime<Utc>, stuck_after_mins: u64, actions: &mut Vec<Action>) {
    if entry.phase == LedgerPhase::Merged {
        if still_registered(entry, observations) {
            push_cleanup(entry, actions);
        }
        return;
    }
    let original = entry.phase;
    apply_inbox(entry, observations, actions);
    apply_pr(entry, observations, actions);
    apply_task_failure(entry, observations, actions);
    if is_worker_phase(entry.phase) {
        apply_session(entry, observations, now, stuck_after_mins, actions);
    }
    if original != LedgerPhase::Merged && entry.phase == LedgerPhase::Merged {
        push_cleanup(entry, actions);
    }
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
    actions: &mut Vec<Action>,
) {
    if entry.phase == LedgerPhase::Merged {
        if still_registered(entry, observations) {
            push_cleanup(entry, actions);
        }
        return;
    }
    apply_pr(entry, observations, actions);
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
fn apply_inbox(entry: &mut LedgerEntry, observations: &Observations, actions: &mut Vec<Action>) {
    let mut reports: Vec<_> = observations
        .inbox
        .iter()
        .filter(|report| matches_entry(report, entry))
        .collect();
    reports.sort_by_key(|report| report.at);
    for report in reports {
        if let Some(task_id) = &report.task_id
            && task_id != &entry.task_id
        {
            entry.task_id.clone_from(task_id);
        }
        match report.kind.as_str() {
            "claimed" if entry.phase == LedgerPhase::Dispatched => {
                entry.phase = LedgerPhase::Claimed
            }
            "turn-done" | "waiting"
                if matches!(entry.phase, LedgerPhase::Dispatched | LedgerPhase::Claimed) =>
            {
                entry.phase = LedgerPhase::Working
            }
            "done" if report.pr_url.is_some() && !terminal(entry.phase) => {
                entry.phase = LedgerPhase::PrOpened;
                entry.pr_url.clone_from(&report.pr_url);
            }
            "blocked" if !terminal(entry.phase) => escalate(
                entry,
                report.reason.as_deref().unwrap_or("worker blocked"),
                actions,
            ),
            "claimed" | "turn-done" | "waiting" | "done" => {}
            kind => actions.push(Action::LogDecision {
                task_id: Some(entry.task_id.clone()),
                message: format!("ignored unknown inbox observation kind {kind:?}"),
            }),
        }
    }
}
fn apply_pr(entry: &mut LedgerEntry, observations: &Observations, actions: &mut Vec<Action>) {
    let task_id = entry.task_id.clone();
    let known_url = entry.pr_url.clone();
    for pr in observations.prs.iter().filter(|pr| {
        pr.task_id.as_deref() == Some(&task_id)
            || pr
                .url
                .as_deref()
                .is_some_and(|url| known_url.as_deref() == Some(url))
    }) {
        match pr.state.to_ascii_uppercase().as_str() {
            "MERGED" => {
                entry.phase = LedgerPhase::Merged;
                if entry.pr_url.is_none() {
                    entry.pr_url.clone_from(&pr.url);
                }
            }
            "OPEN" if !terminal(entry.phase) => {
                entry.phase = LedgerPhase::PrOpened;
                if entry.pr_url.is_none() {
                    entry.pr_url.clone_from(&pr.url);
                }
            }
            "CLOSED" => {}
            state => actions.push(Action::LogDecision {
                task_id: Some(entry.task_id.clone()),
                message: format!("ignored unknown PR state {state:?}"),
            }),
        }
    }
}
#[rustfmt::skip]
fn apply_task_failure(entry: &mut LedgerEntry, observations: &Observations, actions: &mut Vec<Action>) {
    let task_id = entry.task_id.clone();
    for task in observations
        .tasks
        .iter()
        .filter(|task| task.task_id == task_id)
    {
        match task.state.as_str() {
            "open" if matches!(entry.phase, LedgerPhase::Claimed | LedgerPhase::Working) => {
                escalate(entry, "worker released its dropr task lock", actions);
            }
            "open" | "in_progress" | "ready" | "closed" => {}
            state => actions.push(Action::LogDecision {
                task_id: Some(entry.task_id.clone()),
                message: format!("ignored unknown dropr task state {state:?}"),
            }),
        }
    }
}
#[rustfmt::skip]
fn apply_session(entry: &mut LedgerEntry, observations: &Observations, now: DateTime<Utc>, stuck_after_mins: u64, actions: &mut Vec<Action>) {
    let Some(session) = observations
        .sessions
        .iter()
        .find(|s| s.agent_id == entry.agent_id)
    else {
        return;
    };
    if matches!(session.status.as_str(), "dead" | "branch_only") {
        fail(entry, "worker session is dead", FailureOrigin::Worker, actions);
        return;
    }
    if !matches!(
        session.status.as_str(),
        "running" | "waiting" | "done" | "idle"
    ) {
        actions.push(Action::LogDecision {
            task_id: Some(entry.task_id.clone()),
            message: format!("ignored unknown session status {:?}", session.status),
        });
        return;
    }
    let last = session
        .last_activity_at
        .or_else(|| (entry.phase == LedgerPhase::Dispatched).then_some(entry.dispatched_at));
    if last.is_some_and(|last| {
        now.signed_duration_since(last) > Duration::minutes(stuck_after_mins as i64)
    }) {
        fail(entry, "worker exceeded stuck timeout", FailureOrigin::Worker, actions);
    }
}
#[rustfmt::skip]
fn fail(entry: &mut LedgerEntry, reason: &str, origin: FailureOrigin, actions: &mut Vec<Action>) {
    entry.phase = LedgerPhase::Failed;
    actions.push(Action::MarkFailed { task_id: entry.task_id.clone(), reason: reason.into(), origin });
    actions.push(Action::Notify { message: format!("{}: {reason}", entry.display_id) });
    actions.push(Action::LogDecision { task_id: Some(entry.task_id.clone()), message: reason.into() });
}
fn escalate(entry: &mut LedgerEntry, reason: &str, actions: &mut Vec<Action>) {
    entry.phase = LedgerPhase::Escalated;
    actions.push(Action::Escalate {
        task_id: entry.task_id.clone(),
        reason: reason.into(),
    });
    actions.push(Action::Notify {
        message: format!("{}: {reason}", entry.display_id),
    });
    actions.push(Action::LogDecision {
        task_id: Some(entry.task_id.clone()),
        message: reason.into(),
    });
}
#[rustfmt::skip]
fn matches_entry(report: &InboxObservation, entry: &LedgerEntry) -> bool { report.agent_id == entry.agent_id || report.task_id.as_deref() == Some(entry.task_id.as_str()) }
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
        .map(|message| Action::LogDecision {
            task_id: None,
            message: format!("observation skipped: {message}"),
        })
        .collect()
}
#[cfg(test)]
#[path = "monitor_tests.rs"]
mod tests;
