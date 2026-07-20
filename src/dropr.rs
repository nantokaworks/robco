use std::{collections::HashMap, collections::HashSet, process::Command, time::Duration};

use serde::{Deserialize, Serialize};

mod mcp;

const WORKSPACE_LIST_TIMEOUT: Duration = Duration::from_secs(3);

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DroprWorkspace {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub repo_url: String,
}

#[derive(Debug, Default, Clone)]
pub struct DroprOverlay {
    by_canonical_repo: HashMap<String, DroprWorkspace>,
}

impl DroprOverlay {
    pub fn load_best_effort() -> Self {
        Self::load_with_status_timeout(WORKSPACE_LIST_TIMEOUT).0
    }

    /// Load the workspace overlay, also reporting whether the
    /// `dropr workspace list` invocation succeeded, so callers can tell
    /// "no workspaces" apart from "dropr CLI unavailable or failing".
    pub fn load_with_status_timeout(timeout: Duration) -> (Self, bool) {
        let mut command = Command::new("dropr");
        command.args(["workspace", "list"]);
        match crate::overseer::exec::run_timeout(command, timeout) {
            Ok(output) if output.status.success() => {
                (Self::from_workspace_list(&output.stdout), true)
            }
            _ => (Self::default(), false),
        }
    }

    fn from_workspace_list(raw: &[u8]) -> Self {
        let stdout = String::from_utf8_lossy(raw);
        let mut by_canonical_repo = HashMap::new();
        for line in stdout.lines().skip(1) {
            if let Some(workspace) = parse_workspace_line(line)
                && let Some(canonical) = canonical_repo(&workspace.repo_url)
            {
                by_canonical_repo.insert(canonical, workspace);
            }
        }
        Self { by_canonical_repo }
    }

    pub fn find_by_repo_url(&self, repo_url: &str) -> Option<&DroprWorkspace> {
        canonical_repo(repo_url).and_then(|key| self.by_canonical_repo.get(&key))
    }
}

pub fn fetch_ready_tasks(workspace_id: &str, limit: usize) -> Option<Vec<DroprTaskCandidate>> {
    fetch_ready_as(workspace_id, limit)
}

pub fn fetch_ready_dispatch_tasks_timeout(
    workspace_id: &str,
    limit: usize,
    timeout: Duration,
) -> std::result::Result<Vec<DroprDispatchCandidate>, ReadyDispatchError> {
    let limit = limit.to_string();
    let mut command = Command::new("dropr");
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

fn fetch_ready_as<T: for<'de> Deserialize<'de>>(
    workspace_id: &str,
    limit: usize,
) -> Option<Vec<T>> {
    let limit = limit.to_string();
    fetch_as(&[
        "task",
        "ready",
        "--workspace",
        workspace_id,
        "--limit",
        &limit,
        "--json",
    ])
}

pub fn fetch_in_progress_tasks(workspace_id: &str) -> Option<Vec<DroprTaskCandidate>> {
    mcp::fetch_in_progress_tasks(workspace_id)
}

pub fn fetch_repo_tasks(workspace_id: &str) -> Option<Vec<DroprTaskCandidate>> {
    merge_repo_tasks(
        fetch_in_progress_tasks(workspace_id),
        fetch_ready_tasks(workspace_id, 3),
    )
}

pub(crate) fn scribble_create_timeout(
    task_id: &str,
    content: &str,
    timeout: Duration,
) -> crate::Result<()> {
    let mut command = Command::new("dropr");
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
    let mut command = Command::new("dropr");
    command.args(["task", "status", "update", task_id, status]);
    checked_timeout(command, timeout, "dropr task status update")
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

fn fetch_as<T: for<'de> Deserialize<'de>>(args: &[&str]) -> Option<Vec<T>> {
    let output = Command::new("dropr").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_as(&output.stdout)
}

fn merge_repo_tasks(
    in_progress: Option<Vec<DroprTaskCandidate>>,
    ready: Option<Vec<DroprTaskCandidate>>,
) -> Option<Vec<DroprTaskCandidate>> {
    if in_progress.is_none() && ready.is_none() {
        return None;
    }
    let mut tasks = in_progress.unwrap_or_default();
    for task in &mut tasks {
        if task.status.is_empty() {
            task.status = "in_progress".to_string();
        }
    }
    tasks.extend(ready.unwrap_or_default());
    let mut seen = HashSet::new(); // keep first occurrence: in-progress copy wins over ready dup
    tasks.retain(|task| seen.insert(task.display_id.clone()));
    Some(tasks)
}

pub(super) fn parse_tasks(raw: &[u8]) -> Option<Vec<DroprTaskCandidate>> {
    parse_as(raw)
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

fn parse_workspace_line(line: &str) -> Option<DroprWorkspace> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let repo_start = trimmed
        .find("http://")
        .or_else(|| trimmed.find("https://"))?;
    let (left, repo_url) = trimmed.split_at(repo_start);
    let mut parts = left.split_whitespace();
    let kind = parts.next()?.to_string();
    let id = parts.next()?.to_string();
    let name = parts.collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return None;
    }

    Some(DroprWorkspace {
        kind,
        id,
        name,
        repo_url: repo_url.trim().to_string(),
    })
}

pub fn canonical_repo(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches(".git");
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        return Some(format!("github:{}", rest.to_ascii_lowercase()));
    }
    for prefix in [
        "https://github.com/",
        "http://github.com/",
        "ssh://git@github.com/",
    ] {
        if let Some(rest) = url.strip_prefix(prefix) {
            return Some(format!("github:{}", rest.to_ascii_lowercase()));
        }
    }
    None
}

#[cfg(test)]
#[path = "dropr_tests.rs"]
mod tests;
