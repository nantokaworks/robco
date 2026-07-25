use std::{process::Command, time::Duration};

use serde::Deserialize;

mod claim;
mod mcp;
mod repo_tasks;
mod workspace;

pub use claim::{ClaimAttempt, TaskClaim, claim_task, release_claim, task_claim};
pub use repo_tasks::{DroprTaskFetch, FETCH_BUDGET, TASK_FETCH_LIMIT};
pub use workspace::{DroprOverlay, DroprWorkspace, canonical_repo};

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
    command.args([
        "task",
        "ready",
        "--workspace",
        workspace_id,
        "--limit",
        &limit,
        "--json",
    ]);
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

pub(crate) fn scribble_create_timeout(
    task_id: &str,
    content: &str,
    timeout: Duration,
) -> crate::Result<()> {
    let mut command = dropr_command("dropr scribble create")?;
    command.args([
        "scribble",
        "create",
        "--task",
        task_id,
        "--content",
        content,
    ]);
    checked_timeout(command, timeout, "dropr scribble create")
}

pub(crate) fn task_status_update_timeout(
    task_id: &str,
    status: &str,
    timeout: Duration,
) -> crate::Result<()> {
    let mut command = dropr_command("dropr task status update")?;
    command.args(["task", "status", "update", task_id, status]);
    checked_timeout(command, timeout, "dropr task status update")
}

fn dropr_command(context: &'static str) -> crate::Result<Command> {
    crate::config::resolve_program("dropr")
        .map(Command::new)
        .ok_or_else(|| crate::Error::Command {
            context,
            stderr: "dropr binary not found on PATH or common install dirs; install dropr or add it to the overseer daemon's PATH".into(),
        })
}

fn checked_timeout(
    command: Command,
    timeout: Duration,
    context: &'static str,
) -> crate::Result<()> {
    let output = crate::overseer::exec::run_timeout(command, timeout)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(crate::Error::Command {
            context,
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
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
