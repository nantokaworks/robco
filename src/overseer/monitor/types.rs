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
    pub branches: Vec<BranchObservation>,
    pub errors: Vec<ObservationError>,
    /// Agents that still hold a ledger entry but are no longer Overseer
    /// children. Their entries are dropped rather than reconciled — see
    /// [`crate::overseer::monitor`].
    pub detached_agents: Vec<String>,
}
/// A probe that could not be completed.
///
/// The subject is carried alongside the message because the decision this
/// becomes is otherwise unattributable: `decisions.jsonl` used to record every
/// probe failure with no task and no repository, so an operator could see that
/// something failed but never which entry it failed for.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ObservationError {
    pub message: String,
    pub task_id: Option<String>,
    pub repo: Option<String>,
}
impl ObservationError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            task_id: None,
            repo: None,
        }
    }
    /// Names the ledger entry the failed probe belongs to.
    pub fn about(mut self, task_id: &str, repo: &str) -> Self {
        self.task_id = Some(task_id.to_owned());
        self.repo = Some(repo.to_owned());
        self
    }
    /// Names the repository a repository-wide probe failed for.
    pub fn in_repo(mut self, repo: &str) -> Self {
        self.repo = Some(repo.to_owned());
        self
    }
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
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionObservation {
    pub agent_id: String,
    pub status: String,
    pub last_activity_at: Option<DateTime<Utc>>,
}
/// `TaskObservation::state` sentinel for a task probe that succeeded but
/// found no task under this entry's display id — the dropr task itself is
/// gone, not merely unobserved this pass. Distinct from an ordinary dropr
/// status string so a reader can tell "confirmed absent" apart from every
/// value dropr itself would ever report. Read by
/// `daemon::retention::beyond_reach` to sweep an escalated entry nothing can
/// act on any more; harmless everywhere else `TaskObservation` is read,
/// which treats it as just another quiet state.
pub const TASK_ABSENT: &str = "absent";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskObservation {
    pub task_id: String,
    pub state: String,
    /// When dropr last recorded a change to this task. `None` when the
    /// fetch's source did not carry the field. Read by
    /// `monitor::apply::apply_escalation_resolution` to tell a task dropr
    /// touched after an entry escalated from one that has sat unchanged.
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

/// The task branch's own local commit history, read straight off the entry's
/// worktree. A new commit on it after an entry escalated is worker activity
/// exactly like fresh tmux input — the worker pushed from the same worktree
/// this observation reads — so `monitor::apply::apply_escalation_resolution`
/// treats it as an equivalent signal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BranchObservation {
    pub task_id: String,
    pub latest_commit_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct PrObservation {
    pub task_id: Option<String>,
    pub url: Option<String>,
    pub state: String,
    pub status_check_rollup: Vec<serde_json::Value>,
    /// When GitHub reports the pull request merged. `None` for one that never
    /// merged, and for a read from a `gh` build too old to report the field.
    /// The reconcile pass reads this to tell a merge that just happened from
    /// one it is only now discovering — see `daemon::discord_events`.
    pub merged_at: Option<DateTime<Utc>>,
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
    /// Considers whether the pull request that just merged should trigger the
    /// local release pipeline. Pushed once, the pass an entry first reaches
    /// `Merged`, and carries no verdict of its own — every guard (repo, task
    /// scope, checkout readiness) is evaluated where the process actually
    /// runs. See `overseer::release_pipeline::consider`.
    CheckReleasePipeline { task_id: String, repo: String, pr_url: Option<String> },
}
