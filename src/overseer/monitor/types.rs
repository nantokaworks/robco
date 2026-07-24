use crate::overseer::inbox::InboxReport;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FailureOrigin {
    Worker,
    Infra,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Observations {
    pub inbox: Vec<InboxObservation>,
    pub registered_agents: Vec<String>,
    pub sessions: Vec<SessionObservation>,
    pub tasks: Vec<TaskObservation>,
    pub prs: Vec<PrObservation>,
    pub errors: Vec<String>,
    pub manual_agents: Vec<String>,
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
    /// Runs the post-merge cleanup for one agent: the base fast-forward, the
    /// worktree removal, and the branch deletion. It carries no policy — the
    /// merge gate that produced it already decided the work landed.
    RemoveWorktree { agent_id: String },
    MarkFailed { task_id: String, reason: String, origin: FailureOrigin },
    Escalate { task_id: String, reason: String },
    Notify { message: String },
    LogDecision { task_id: Option<String>, message: String },
}
