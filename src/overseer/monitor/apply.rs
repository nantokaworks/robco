//! What each observation stream does to a ledger entry.
//!
//! One function per source — the worker inbox, the pull request, the dropr
//! task, the tmux session — so the phase a pass lands on can be traced back to
//! the single stream that moved it. The pass itself lives in the parent module;
//! these are the steps it applies, in order, to one entry.
//!
//! Every arm that cannot map an observation onto a phase logs a decision rather
//! than guessing: an unknown kind is a newer robco talking to an older one, and
//! the ledger is safer standing still than acting on a value it does not know.

use chrono::{DateTime, Duration, Utc};

use super::{
    Action, FailureOrigin, InboxObservation, LedgerEntry, LedgerPhase, Observations, terminal,
};

pub(super) fn apply_inbox(
    entry: &mut LedgerEntry,
    observations: &Observations,
    actions: &mut Vec<Action>,
) {
    let mut reports: Vec<_> = observations
        .inbox
        .iter()
        .filter(|report| matches_entry(report, entry))
        .collect();
    reports.sort_by_key(|report| report.at);
    for report in reports {
        match report.kind.as_str() {
            "claimed" if entry.phase == LedgerPhase::Dispatched => {
                entry.phase = LedgerPhase::Claimed
            }
            "turn-done" | "waiting"
                if matches!(entry.phase, LedgerPhase::Dispatched | LedgerPhase::Claimed) =>
            {
                entry.phase = LedgerPhase::Working
            }
            "blocked" if !terminal(entry.phase) => escalate(entry, "worker blocked", actions),
            "claimed" | "turn-done" | "waiting" | "done" => {}
            kind => actions.push(Action::LogDecision {
                task_id: Some(entry.task_id.clone()),
                message: format!("ignored unknown inbox observation kind {kind:?}"),
            }),
        }
    }
}
pub(super) fn apply_pr(
    entry: &mut LedgerEntry,
    observations: &Observations,
    actions: &mut Vec<Action>,
) {
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
                // The recorded URL is a hint, not the key: a terminal entry's
                // pull request is matched by branch (see
                // `daemon::external_state::probe_by_branch`), so the URL that
                // actually merged can differ from whatever was last recorded.
                // Correct it unconditionally rather than only filling a gap.
                if entry.pr_url.as_deref() != pr.url.as_deref() {
                    entry.pr_url.clone_from(&pr.url);
                }
            }
            "OPEN" if !terminal(entry.phase) => {
                entry.phase = LedgerPhase::PrOpened;
                if entry.pr_url.is_none() {
                    entry.pr_url.clone_from(&pr.url);
                }
            }
            // Neither moves an entry that is already terminal. An escalated entry
            // is read again so a hand-merge can reach it, and its pull request is
            // usually still open while the operator looks — a state, not a
            // surprise, and the arm below would report it every poll interval.
            "OPEN" | "CLOSED" => {}
            state => actions.push(Action::LogDecision {
                task_id: Some(entry.task_id.clone()),
                message: format!("ignored unknown PR state {state:?}"),
            }),
        }
    }
}
#[rustfmt::skip]
pub(super) fn apply_task_failure(entry: &mut LedgerEntry, observations: &Observations, actions: &mut Vec<Action>) {
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
pub(super) fn apply_session(entry: &mut LedgerEntry, observations: &Observations, now: DateTime<Utc>, stuck_after_mins: u64, actions: &mut Vec<Action>) {
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
fn matches_entry(report: &InboxObservation, entry: &LedgerEntry) -> bool {
    report.agent_id == entry.agent_id
}
