use std::path::PathBuf;

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::subagents::TaskSubagent;

use super::Status;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentNode {
    pub id: String,
    #[serde(default)]
    pub parent_agent_id: Option<String>,
    pub title: String,
    /// The bare dropr task number (e.g. `"333"`) an Overseer-dispatched worker
    /// was created for, captured once at spawn time from the naming slug so
    /// the tree row can lead with it without re-deriving it from `branch` or
    /// `title` at render time. `None` for a manually-created or adopted agent,
    /// which carries no dropr task number at all.
    #[serde(default)]
    pub task_number: Option<String>,
    pub worktree_path: PathBuf,
    pub branch: String,
    pub base_commit: String,
    pub program: String,
    /// The `robco` version that performed this spawn — the version whose
    /// compiled-in template `agent::hooks::write_report_hooks` wrote from.
    /// `None` for an agent adopted from a worktree robco did not create
    /// itself, which was never spawned by any robco binary at all
    /// (dropr:559).
    #[serde(default)]
    pub spawned_by_version: Option<String>,
    #[serde(default)]
    pub claude_session_id: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
    pub tmux_session: String,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
    #[serde(skip)]
    pub status: Status,
    /// Whether the live AI session's worktree directory is missing. Runtime
    /// only; orthogonal to the captured AI status.
    #[serde(skip)]
    pub worktree_missing: bool,
    /// Detail from the latest failed native merge attempt. Runtime only.
    #[serde(skip)]
    pub merge_error: Option<String>,
    #[serde(skip)]
    pub last_capture: Option<String>,
    /// Last observed spinner frame for motion detection.
    #[serde(skip)]
    pub last_spinner: Option<String>,
    #[serde(skip)]
    pub last_change_at: Option<DateTime<Local>>,
    #[serde(skip)]
    pub last_auto_accept_at: Option<DateTime<Local>>,
    /// Whether the agent's companion shell (TERM) session is running a
    /// foreground command. Runtime only; refreshed each tick, never persisted.
    #[serde(skip)]
    pub shell_working: bool,
    /// Whether the AI session has an in-flight tool call. Runtime only;
    /// refreshed each tick and never persisted.
    #[serde(skip)]
    pub mcp_active: bool,
    #[serde(skip)]
    pub pane_pid: Option<u32>,
    #[serde(skip)]
    pub tracked_command: Option<String>,
    #[serde(skip)]
    pub subagents: Vec<TaskSubagent>,
    #[serde(skip)]
    pub children: Vec<ChildWorktree>,
}

#[derive(Debug, Clone)]
pub struct ChildWorktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub clean: Option<bool>,
    pub ahead_behind: Option<(u32, u32)>,
    pub tmux_session: Option<String>,
    pub modified_at: Option<DateTime<Local>>,
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
