use super::{
    inbox::InboxReport,
    ledger::{Ledger, LedgerEntry, LedgerPhase},
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Observations {
    pub inbox: Vec<InboxObservation>,
    pub registered_agents: Vec<String>,
    pub sessions: Vec<SessionObservation>,
    pub tasks: Vec<TaskObservation>,
    pub prs: Vec<PrObservation>,
    pub errors: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObservationSnapshot {
    pub at: DateTime<Utc>,
    pub observations: Observations,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InboxObservation {
    pub at: DateTime<Utc>,
    pub agent_id: String,
    pub kind: String,
    pub task_id: Option<String>,
    pub pr_url: Option<String>,
    pub reason: Option<String>,
}
impl From<InboxReport> for InboxObservation {
    fn from(report: InboxReport) -> Self {
        Self {
            at: report.at,
            agent_id: report.agent_id,
            kind: serde_json::to_value(report.kind)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "unknown".into()),
            task_id: report.task_id,
            pr_url: report.pr_url,
            reason: report.reason,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionObservation {
    pub agent_id: String,
    pub status: String,
    pub last_activity_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskObservation {
    pub task_id: String,
    pub state: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct PrObservation {
    pub task_id: Option<String>,
    pub url: Option<String>,
    pub state: String,
    pub status_check_rollup: Vec<serde_json::Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[rustfmt::skip]
pub enum Action {
    KillSession { agent_id: String },
    RemoveWorktree { agent_id: String, keep_branch: bool },
    MarkFailed { task_id: String, reason: String },
    Escalate { task_id: String, reason: String },
    Notify { message: String },
    LogDecision { task_id: Option<String>, message: String },
}
#[rustfmt::skip]
pub fn reconcile(ledger: &Ledger, observations: &Observations, now: DateTime<Utc>, stuck_after_mins: u64) -> (Ledger, Vec<Action>) {
    let mut next = ledger.clone();
    let mut actions = observation_errors(observations);
    for entry in &mut next.entries {
        reconcile_entry(entry, observations, now, stuck_after_mins, &mut actions);
    }
    (next, actions)
}
#[rustfmt::skip]
fn reconcile_entry(entry: &mut LedgerEntry, observations: &Observations, now: DateTime<Utc>, stuck_after_mins: u64, actions: &mut Vec<Action>) {
    if entry.phase == LedgerPhase::Merged {
        if observations
            .registered_agents
            .iter()
            .any(|agent_id| agent_id == &entry.agent_id)
        {
            actions.push(Action::KillSession {
                agent_id: entry.agent_id.clone(),
            });
            actions.push(Action::RemoveWorktree {
                agent_id: entry.agent_id.clone(),
                keep_branch: true,
            });
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
        actions.push(Action::KillSession {
            agent_id: entry.agent_id.clone(),
        });
        actions.push(Action::RemoveWorktree {
            agent_id: entry.agent_id.clone(),
            keep_branch: true,
        });
    }
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
        fail(entry, "worker session is dead", actions);
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
        fail(entry, "worker exceeded stuck timeout", actions);
    }
}
fn fail(entry: &mut LedgerEntry, reason: &str, actions: &mut Vec<Action>) {
    entry.phase = LedgerPhase::Failed;
    actions.push(Action::MarkFailed {
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
    report.agent_id == entry.agent_id || report.task_id.as_deref() == Some(entry.task_id.as_str())
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
        .map(|message| Action::LogDecision {
            task_id: None,
            message: format!("observation skipped: {message}"),
        })
        .collect()
}
#[cfg(test)]
#[path = "monitor_tests.rs"]
mod tests;
