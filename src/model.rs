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
    #[serde(skip)]
    pub main_pane_pid: Option<u32>,
    #[serde(skip)]
    pub main_tracked_command: Option<String>,
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
    #[serde(skip)]
    pub pane_pid: Option<u32>,
    #[serde(skip)]
    pub tracked_command: Option<String>,
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
    /// The tmux session is alive but the worktree directory was removed.
    Orphaned,
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
            Status::Orphaned => "orphan",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orphaned_status_badge_is_orphan() {
        assert_eq!(Status::Orphaned.badge(), "orphan");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    Repo(usize),
    Agent {
        repo: usize,
        agent: usize,
    },
    ChildWorktree {
        repo: usize,
        agent: usize,
        child: usize,
    },
    /// Collapsible header of the "other locations" section listing repos that
    /// live outside the launch directory but still have agents.
    OtherHeader,
    /// Collapsible header of the "orphan sessions" section listing
    /// robco-prefixed tmux sessions no tracked agent or repo accounts for.
    OrphanHeader,
    /// One orphan session row, indexing into [`crate::ui::App`]'s orphan list.
    Orphan(usize),
}

/// A live robco-prefixed tmux session that neither a tracked agent (or its
/// `-shell` twin) nor a registry repo's derived main session accounts for —
/// e.g. left behind by a pre-#66 registry wipe or a deleted worktree. Runtime
/// only; rebuilt from `tmux` on each discovery tick and never persisted.
#[derive(Debug, Clone)]
pub struct OrphanSession {
    pub name: String,
    pub cwd: PathBuf,
}
