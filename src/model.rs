use std::path::PathBuf;

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::{
    dropr::{DroprTaskCandidate, DroprWorkspace},
    subagents::TaskSubagent,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoNode {
    pub path: PathBuf,
    pub name: String,
    pub remote_url: Option<String>,
    /// Persisted manual registration; keeps an agent-less repo listed.
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub agents: Vec<AgentNode>,
    #[serde(skip)]
    pub dropr: Option<DroprWorkspace>,
    #[serde(skip)]
    pub dropr_tasks: Vec<DroprTaskCandidate>,
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
    #[serde(skip)]
    pub main_subagents_active: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentNode {
    pub id: String,
    #[serde(default)]
    pub parent_agent_id: Option<String>,
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
    /// Whether the live AI session's worktree directory is missing. Runtime
    /// only; orthogonal to the captured AI status.
    #[serde(skip)]
    pub worktree_missing: bool,
    /// Detail from the latest failed native merge attempt. Runtime only.
    #[serde(skip)]
    pub merge_error: Option<String>,
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
    pub subagents: Vec<TaskSubagent>,
    #[serde(skip)]
    pub children: Vec<ChildWorktree>,
}

/// Agent indices and identity-tree depths in display order.
pub fn agent_order(agents: &[AgentNode]) -> Vec<(usize, usize)> {
    use std::collections::{HashMap, HashSet};

    let by_id: HashMap<&str, usize> = agents
        .iter()
        .enumerate()
        .map(|(index, agent)| (agent.id.as_str(), index))
        .collect();
    let mut children = vec![Vec::new(); agents.len()];
    for (index, agent) in agents.iter().enumerate() {
        if let Some(parent) = agent
            .parent_agent_id
            .as_deref()
            .and_then(|id| by_id.get(id).copied())
        {
            children[parent].push(index);
        }
    }

    fn visit(
        index: usize,
        depth: usize,
        children: &[Vec<usize>],
        visited: &mut HashSet<usize>,
        ordered: &mut Vec<(usize, usize)>,
    ) {
        if !visited.insert(index) {
            return;
        }
        ordered.push((index, depth));
        for &child in &children[index] {
            visit(child, depth + 1, children, visited, ordered);
        }
    }

    let mut visited = HashSet::new();
    let mut ordered = Vec::with_capacity(agents.len());
    for (index, agent) in agents.iter().enumerate() {
        let known_parent = agent
            .parent_agent_id
            .as_deref()
            .is_some_and(|id| by_id.contains_key(id));
        if !known_parent {
            visit(index, 0, &children, &mut visited, &mut ordered);
        }
    }
    for index in 0..agents.len() {
        visit(index, 0, &children, &mut visited, &mut ordered);
    }
    ordered
}

pub fn agent_depth(agents: &[AgentNode], index: usize) -> usize {
    agent_order(agents)
        .into_iter()
        .find_map(|(candidate, depth)| (candidate == index).then_some(depth))
        .unwrap_or(0)
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

    pub fn glyph(self) -> &'static str {
        match self {
            Status::Idle => "·",
            Status::Running => "▶",
            Status::Waiting => "?",
            Status::Done => "✓",
            Status::Dead => "✗",
            Status::BranchOnly => "⎇",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_badges_and_glyphs_are_stable() {
        assert_eq!(Status::Running.badge(), "run");
        assert_eq!(Status::Running.glyph(), "▶");
        assert_eq!(Status::Waiting.glyph(), "?");
        assert_eq!(Status::Done.glyph(), "✓");
        assert_eq!(Status::Idle.glyph(), "·");
        assert_eq!(Status::Dead.glyph(), "✗");
        assert_eq!(Status::BranchOnly.glyph(), "⎇");
    }

    #[test]
    fn agent_order_nests_children_and_keeps_cycles_visible() {
        fn agent(id: &str, parent: Option<&str>) -> AgentNode {
            let now = Local::now();
            AgentNode {
                id: id.into(),
                parent_agent_id: parent.map(str::to_string),
                title: id.into(),
                worktree_path: PathBuf::from(id),
                branch: id.into(),
                base_commit: String::new(),
                program: String::new(),
                profile: None,
                tmux_session: id.into(),
                created_at: now,
                updated_at: now,
                status: Status::Idle,
                worktree_missing: false,
                merge_error: None,
                last_capture: None,
                last_change_at: None,
                last_auto_accept_at: None,
                shell_working: false,
                pane_pid: None,
                tracked_command: None,
                subagents: Vec::new(),
                children: Vec::new(),
            }
        }
        let agents = vec![
            agent("parent", None),
            agent("other", None),
            agent("child", Some("parent")),
        ];
        assert_eq!(agent_order(&agents), vec![(0, 0), (2, 1), (1, 0)]);

        let cycle = vec![agent("a", Some("b")), agent("b", Some("a"))];
        assert_eq!(agent_order(&cycle), vec![(0, 0), (1, 1)]);

        let self_parent = vec![agent("self", Some("self")), agent("child", Some("self"))];
        assert_eq!(agent_order(&self_parent), vec![(0, 0), (1, 1)]);
    }

    #[test]
    fn merge_error_is_not_persisted() {
        let now = Local::now();
        let agent = AgentNode {
            id: "agent".into(),
            parent_agent_id: None,
            title: "task".into(),
            worktree_path: "/tmp/task".into(),
            branch: "task".into(),
            base_commit: String::new(),
            program: "claude".into(),
            profile: None,
            tmux_session: "robco_task".into(),
            created_at: now,
            updated_at: now,
            status: Status::Idle,
            worktree_missing: false,
            merge_error: Some("merge failed".into()),
            last_capture: None,
            last_change_at: None,
            last_auto_accept_at: None,
            shell_working: false,
            pane_pid: None,
            tracked_command: None,
            subagents: Vec::new(),
            children: Vec::new(),
        };

        let json = serde_json::to_string(&agent).unwrap();
        assert!(!json.contains("merge_error"));
        let restored: AgentNode = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.merge_error, None);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    Overseer,
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
