pub mod claude;

use std::{path::Path, time::SystemTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentStatus {
    Running,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSubagent {
    pub id: String,
    pub agent_type: String,
    pub description: String,
    pub spawn_depth: u32,
    pub started_at: SystemTime,
    pub last_activity_at: SystemTime,
    pub status: SubagentStatus,
}

/// Passive adapter boundary for coding-agent-specific subagent state.
pub trait SubagentReader {
    fn read(&self, worktree_path: &Path, now: SystemTime) -> Vec<TaskSubagent>;
}
