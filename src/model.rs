use std::path::PathBuf;

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::dropr::DroprWorkspace;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoNode {
    pub path: PathBuf,
    pub name: String,
    pub remote_url: Option<String>,
    #[serde(default)]
    pub agents: Vec<AgentNode>,
    #[serde(skip)]
    pub dropr: Option<DroprWorkspace>,
    /// Status of the repo's own main-worktree AI session, or `None` when no such
    /// session is running (the main worktree does not auto-launch one). Runtime
    /// only; refreshed each tick and never persisted.
    #[serde(skip)]
    pub main_status: Option<Status>,
    #[serde(skip)]
    pub main_last_capture: Option<String>,
    #[serde(skip)]
    pub main_last_change_at: Option<DateTime<Local>>,
    /// Whether the repo main-worktree companion shell (TERM) session is running
    /// a foreground command. Runtime only; refreshed each tick, never persisted.
    #[serde(skip)]
    pub main_shell_working: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentNode {
    pub id: String,
    pub title: String,
    pub worktree_path: PathBuf,
    pub branch: String,
    pub base_commit: String,
    pub program: String,
    #[serde(default)]
    pub profile: Option<String>,
    pub tmux_session: String,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
    #[serde(skip)]
    pub status: Status,
    #[serde(skip)]
    pub last_capture: Option<String>,
    #[serde(skip)]
    pub last_change_at: Option<DateTime<Local>>,
    #[serde(skip)]
    pub last_auto_accept_at: Option<DateTime<Local>>,
    /// Whether the agent's companion shell (TERM) session is running a
    /// foreground command. Runtime only; refreshed each tick, never persisted.
    #[serde(skip)]
    pub shell_working: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    Idle,
    Running,
    Waiting,
    /// The AI finished a turn and is sitting at its input prompt with nothing
    /// pending — distinct from `Waiting` (a real y/n / selection prompt) and
    /// from `Idle` (a session that has done nothing yet).
    Done,
    Dead,
    BranchOnly,
}

impl Status {
    pub fn badge(self) -> &'static str {
        match self {
            Status::Idle => "idle",
            Status::Running => "run",
            Status::Waiting => "wait",
            Status::Done => "done",
            Status::Dead => "dead",
            Status::BranchOnly => "branch",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    Repo(usize),
    Agent { repo: usize, agent: usize },
}
