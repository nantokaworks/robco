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
    Action, FailureOrigin, InboxObservation, LedgerEntry, LedgerPhase, Observations, TASK_ABSENT,
    terminal,
};

#[path = "apply_resolution.rs"]
mod resolution;
pub(super) use resolution::apply_escalation_resolution;
use resolution::resolve;

/// Reason recorded when a prerequisite wait outlives its bound — a worker's
/// `waiting-prerequisite` report, or the auto-merge gate's own
/// `daemon::merge_dependency` hold, that never cleared. Named apart from the
/// merge gate's own `prerequisite_unmerged:<display_id>` reason: this one
/// fires from `monitor::reconcile`, which has no pull request read to name
/// the prerequisite from, only the ledger entry's own elapsed wait.
const PREREQUISITE_WAIT_EXCEEDED: &str = "prerequisite_wait_exceeded";

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
            // The worker discovered mid-implementation that this task is
            // ordered behind another and created the dropr `blocks` edge
            // itself (dropr:375) rather than asking the task to be marked
            // `blocked`. Recording the wait — not escalating — is the whole
            // point: dropr's own ready feed excludes the task until the
            // prerequisite closes, so nothing here needs an operator. The
            // phase is deliberately left alone: this entry's worker is
            // stepping aside, not advancing, and `ledger::holds_capacity`
            // (not the phase) is what tells dispatch this entry is no longer
            // active, so a fresh dispatch onto the same task — once dropr
            // offers it again — is a new ledger entry under a new agent id,
            // never a further report on this one.
            "waiting-prerequisite" if !terminal(entry.phase) => {
                entry.prerequisite_wait.get_or_insert(report.at);
            }
            // A worker that got its answer straight from a human typing into
            // its own tmux session — rather than through dropr or the inbox —
            // has no other way to tell Overseer the block lifted. Resolving
            // here, inside `apply_inbox`, means the same pass's `apply_pr` /
            // `apply_task_failure` / `apply_session` immediately re-derive the
            // entry's real phase instead of waiting for the next poll.
            //
            // Gated on `worker_escalated` the same as the signal-driven path
            // in `apply_escalation_resolution`: the worker's own worktree and
            // session stay alive through a merge-subsystem escalation too, so
            // a worker that fires this hook without knowing the block is on
            // the merge side, not its own, must not clear it.
            "unblocked" if entry.phase == LedgerPhase::Escalated && entry.worker_escalated => {
                resolve(entry, "explicit_report", actions)
            }
            // The worker's own claim that its work is finished — a report,
            // not proof. `Merged` / `Failed` / `Escalated` stay derived from
            // what is observed (PR state, session state) elsewhere in this
            // pass; this only marks the entry so the row can show "worker
            // says done" apart from "worker still working" while everything
            // still waits on those observed signals.
            "done" => {
                entry.worker_finished_at = Some(report.at);
            }
            "claimed" | "turn-done" | "waiting" | "waiting-prerequisite" | "unblocked" => {}
            kind => actions.push(Action::LogDecision {
                task_id: Some(entry.task_id.clone()),
                message: format!("ignored unknown inbox observation kind {kind:?}"),
            }),
        }
    }
}

/// Escalates a prerequisite wait that has outlived
/// `overseer.max_prerequisite_wait_hours`, whichever path set it: a worker's
/// `waiting-prerequisite` report (see `apply_inbox`), or the auto-merge
/// gate's `daemon::merge_dependency` hold on a `PrOpened` entry. Called for
/// every entry every `monitor::reconcile` pass regardless of phase, since
/// the merge gate runs as a separate pass and this is the one place both
/// paths are read back.
pub(super) fn apply_prerequisite_wait(
    entry: &mut LedgerEntry,
    now: DateTime<Utc>,
    max_wait_hours: u64,
    actions: &mut Vec<Action>,
) {
    let Some(since) = entry.prerequisite_wait else {
        return;
    };
    if terminal(entry.phase) {
        return;
    }
    if now.signed_duration_since(since) > Duration::hours(max_wait_hours as i64) {
        escalate(entry, PREREQUISITE_WAIT_EXCEEDED, actions);
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
            // A worker stepping aside for a prerequisite wait releases its
            // claim on purpose — see `apply_inbox`'s `waiting-prerequisite`
            // arm, which always runs first in `monitor::reconcile_entry` and
            // so has already set `prerequisite_wait` by the time this reads
            // it — so that release must not read as the same abandonment
            // this arm otherwise escalates.
            "open"
                if matches!(entry.phase, LedgerPhase::Claimed | LedgerPhase::Working)
                    && entry.prerequisite_wait.is_none() =>
            {
                escalate(entry, "worker released its dropr task lock", actions);
            }
            // `TASK_ABSENT` is a positive "confirmed gone" signal for an
            // already-escalated entry — see `daemon::external_state` and
            // `daemon::retention::beyond_reach` — not a state to reconcile
            // a live entry's phase against here.
            "open" | "in_progress" | "ready" | "closed" | TASK_ABSENT => {}
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
    // `last_activity_at` carries a real tmux `#{session_activity}` reading for
    // any live session, in every phase. A `None` reading here means this
    // tick's probe faulted (already logged as an `ObservationError` in
    // `daemon::observations::gather`), not that the session sat idle since
    // dispatch; judging it against `dispatched_at` would fail an actively
    // working session on a single transient probe miss. Leave it unjudged
    // this tick instead — the next successful probe carries a real value.
    //
    // The staleness clock floors at `dispatched_at`, not `last_activity_at`
    // alone (dropr:523): `daemon::observations::adopt_registry_children_from`
    // stamps `dispatched_at` from the pass that first watches the entry, so a
    // worker whose last real tmux activity predates that pass — because
    // adoption itself ran late, or the daemon was down — gets a full
    // `stuck_after_mins` window from the moment anyone actually started
    // watching it, instead of being failed on the very first pass for a gap
    // nobody was measuring. A worker that goes quiet after that is still
    // caught: once `dispatched_at` is further in the past than
    // `last_activity_at`, only `last_activity_at` matters, exactly as before.
    if session.last_activity_at.is_some_and(|last| {
        let baseline = entry.dispatched_at.max(last);
        now.signed_duration_since(baseline) > Duration::minutes(stuck_after_mins as i64)
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
    // The one call site allowed to set this: see `LedgerEntry::worker_escalated`.
    entry.worker_escalated = true;
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
