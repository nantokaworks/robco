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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    Idle,
    Running,
    Waiting,
    Dead,
    BranchOnly,
}

impl Status {
    pub fn badge(self) -> &'static str {
        match self {
            Status::Idle => "idle",
            Status::Running => "run",
            Status::Waiting => "wait",
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
