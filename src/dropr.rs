use std::{process::Command, time::Duration};

use chrono::{DateTime, Utc};
use serde::Deserialize;

mod claim;
mod dependency;
mod mcp;
mod repo_tasks;
mod workspace;
mod writes;

pub use claim::{ClaimAttempt, TaskClaim, claim_task, release_claim, task_claim};
pub use dependency::blocking_prerequisite_timeout;
pub use repo_tasks::{DroprTaskFetch, FETCH_BUDGET, TASK_FETCH_LIMIT};
pub use workspace::{DroprOverlay, DroprWorkspace, canonical_repo};
pub use writes::{WriteError, WriteResult};
pub(crate) use writes::{scribble_create_timeout, task_create_timeout, task_status_update_timeout};

/// `dropr task ready --limit` caps at 20; larger values make the CLI
/// exit with an argument error and the fetch fails for every repo.
pub const READY_FETCH_LIMIT: usize = 20;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DroprTaskCandidate {
    #[serde(alias = "global_display_id")]
    pub display_id: String,
    pub title: String,
    #[serde(default)]
    pub priority: String,
    #[serde(default)]
    pub status: String,
    /// dropr's numeric refinement of `priority`, ordering candidates within a
    /// bucket. `None` when the source omitted it or it did not parse as an
    /// integer — `dropr task ready --json` does not emit this field today, so
    /// every candidate from that feed carries `None` until the CLI grows it.
    #[serde(default)]
    pub priority_score: Option<i64>,
    /// The reason text from the task's unresolved `blocker` scribble, when the
    /// task is `blocked` and the fetch found one. `task_list` never returns
    /// this itself — `dropr::repo_tasks` fills it in with a separate lookup
    /// after the task walk settles. `None` for a task that is not blocked, or
    /// when the lookup found or asked nothing.
    #[serde(default)]
    pub blocked_reason: Option<String>,
    /// When dropr last recorded a change to this task — a status move, or any
    /// other field edit. `task_list` returns it on every row already; this is
    /// only the first reader that keeps it, for
    /// `overseer::monitor::apply_escalation_resolution`'s "did the task
    /// change since the entry escalated" signal. `None` for a source that
    /// omits it, such as `dropr task ready --json`.
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DroprDispatchCandidate {
    #[serde(default, alias = "task_id")]
    pub id: String,
    #[serde(flatten)]
    pub task: DroprTaskCandidate,
    #[serde(default, alias = "created_by", alias = "createdBy")]
    pub author: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadyDispatchError {
    Command,
    Exit,
    Parse,
}

impl ReadyDispatchError {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Command | Self::Exit => "ready_fetch_failed",
            Self::Parse => "ready_parse_failed",
        }
    }
}

pub fn fetch_ready_dispatch_tasks_timeout(
    workspace_id: &str,
    limit: usize,
    timeout: Duration,
) -> std::result::Result<Vec<DroprDispatchCandidate>, ReadyDispatchError> {
    let limit = limit.to_string();
    let program = crate::config::resolve_program("dropr").ok_or(ReadyDispatchError::Command)?;
    let mut command = Command::new(program);
    command.args(ready_args(workspace_id, &limit));
    let output = crate::overseer::exec::run_timeout(command, timeout)
        .map_err(|_| ReadyDispatchError::Command)?;
    if !output.status.success() {
        return Err(ReadyDispatchError::Exit);
    }
    parse_as(&output.stdout).ok_or(ReadyDispatchError::Parse)
}

/// The outstanding tasks of one workspace, for display.
///
/// Deliberately not `dropr task ready`: that endpoint nominates dispatch
/// candidates and applies its own selection, so it cannot answer "what tasks
/// does this repository have". See [`repo_tasks`] for how the hierarchy is
/// walked and how a query that did not answer is reported.
pub fn fetch_repo_tasks(workspace_id: &str) -> DroprTaskFetch {
    repo_tasks::fetch(workspace_id)
}

/// The argv `dropr task ready` is invoked with, spelled once so a test can pin
/// it against the CLI surface that exists.
///
/// `task ready` and [`workspace::DroprOverlay`]'s `workspace list` are all the
/// CLI robco still shells out to. The writes in [`writes`] and the reads in
/// [`repo_tasks`] and [`claim`] go over `dropr mcp-stdio` instead, because the
/// CLI never grew the commands they need.
fn ready_args<'a>(workspace_id: &'a str, limit: &'a str) -> [&'a str; 7] {
    [
        "task",
        "ready",
        "--workspace",
        workspace_id,
        "--limit",
        limit,
        "--json",
    ]
}

fn parse_as<T: for<'de> Deserialize<'de>>(raw: &[u8]) -> Option<Vec<T>> {
    let value: serde_json::Value = serde_json::from_slice(raw).ok()?;
    let tasks = match value {
        serde_json::Value::Array(tasks) => tasks,
        serde_json::Value::Object(mut object) => object.remove("tasks")?.as_array()?.clone(),
        _ => return None,
    };
    tasks
        .into_iter()
        .filter_map(|task| serde_json::from_value(task).ok())
        .collect::<Vec<_>>()
        .into()
}

#[cfg(test)]
#[path = "dropr_tests.rs"]
mod tests;
