use std::{collections::BTreeMap, fs};

use serde::{Deserialize, Serialize};

use crate::{
    Result,
    config::{ensure_robco_dir, state_path},
    model::RepoNode,
};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Registry {
    pub version: u32,
    pub repos: Vec<RepoNode>,
}

impl Registry {
    pub fn load() -> Result<Self> {
        let path = state_path()?;
        if !path.exists() {
            return Ok(Self {
                version: 1,
                repos: Vec::new(),
            });
        }
        let raw = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn save(&self) -> Result<()> {
        ensure_robco_dir()?;
        let path = state_path()?;
        let raw = serde_json::to_string_pretty(self)?;
        fs::write(path, raw)?;
        Ok(())
    }

    pub fn merge_discovered(&mut self, discovered: Vec<RepoNode>) {
        let mut known: BTreeMap<String, RepoNode> = self
            .repos
            .drain(..)
            .map(|repo| (repo.path.to_string_lossy().to_string(), repo))
            .collect();

        self.repos = discovered
            .into_iter()
            .map(|mut repo| {
                if let Some(existing) = known.remove(&repo.path.to_string_lossy().to_string()) {
                    // Carry over the tracked agents and runtime status so a
                    // re-scan does not drop worktrees or flicker the repo's
                    // main-session badge. Prefer a freshly-resolved dropr
                    // overlay, falling back to the previous one.
                    repo.pinned = repo.pinned || existing.pinned;
                    repo.agents = existing.agents;
                    repo.main_status = existing.main_status;
                    repo.main_last_capture = existing.main_last_capture;
                    repo.main_last_change_at = existing.main_last_change_at;
                    repo.dropr = repo.dropr.or(existing.dropr);
                }
                repo
            })
            .collect();

        // Repos registered from another launch directory are never in the
        // discovered set. Keep ones that still track agents, plus pinned manual
        // registrations. Agent-less, unpinned leftovers are dropped.
        self.repos.extend(
            known
                .into_values()
                .filter(|repo| !repo.agents.is_empty() || repo.pinned),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::*;
    use crate::model::{AgentNode, RepoNode};
    use crate::subagents::{SubagentStatus, TaskSubagent};

    fn repo(path: &str, agents: Vec<AgentNode>) -> RepoNode {
        RepoNode {
            path: path.into(),
            name: path.rsplit('/').next().unwrap_or("repo").to_string(),
            remote_url: None,
            pinned: false,
            agents,
            dropr: None,
            dropr_tasks: Vec::new(),
            main_status: None,
            main_last_capture: None,
            main_last_change_at: None,
            main_shell_working: false,
            main_pane_pid: None,
            main_tracked_command: None,
            main_subagents_active: 0,
        }
    }

    fn dummy_agent() -> AgentNode {
        let now = chrono::Local::now();
        AgentNode {
            id: "agent123".to_string(),
            title: "t".to_string(),
            worktree_path: "/tmp/wt".into(),
            branch: "b".to_string(),
            base_commit: String::new(),
            program: "claude".to_string(),
            profile: None,
            tmux_session: "robco_r_t".to_string(),
            created_at: now,
            updated_at: now,
            status: Default::default(),
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

    #[test]
    fn merge_keeps_undiscovered_repo_with_agents() {
        let mut registry = Registry {
            version: 1,
            repos: vec![repo("/a/one", vec![dummy_agent()])],
        };
        registry.merge_discovered(vec![repo("/b/two", Vec::new())]);
        let paths: Vec<String> = registry
            .repos
            .iter()
            .map(|repo| repo.path.to_string_lossy().to_string())
            .collect();
        assert_eq!(paths, ["/b/two", "/a/one"]);
        assert_eq!(registry.repos[1].agents.len(), 1);
    }

    #[test]
    fn merge_drops_undiscovered_repo_without_agents() {
        let mut registry = Registry {
            version: 1,
            repos: vec![repo("/a/one", Vec::new())],
        };
        registry.merge_discovered(vec![repo("/b/two", Vec::new())]);
        assert_eq!(registry.repos.len(), 1);
        assert_eq!(registry.repos[0].path.to_string_lossy(), "/b/two");
    }

    #[test]
    fn merge_keeps_undiscovered_pinned_repo_without_agents() {
        let mut pinned = repo("/a/one", Vec::new());
        pinned.pinned = true;
        let mut registry = Registry {
            version: 1,
            repos: vec![pinned],
        };

        registry.merge_discovered(vec![repo("/b/two", Vec::new())]);

        assert_eq!(registry.repos.len(), 2);
        assert_eq!(registry.repos[1].path.to_string_lossy(), "/a/one");
        assert!(registry.repos[1].pinned);
    }

    #[test]
    fn runtime_fields_are_not_serialized_and_default_when_absent() {
        let mut agent = dummy_agent();
        agent.subagents.push(TaskSubagent {
            id: "worker".into(),
            agent_type: "Explore".into(),
            description: "inspect".into(),
            spawn_depth: 1,
            started_at: SystemTime::UNIX_EPOCH,
            last_activity_at: SystemTime::UNIX_EPOCH,
            status: SubagentStatus::Running,
        });
        agent.children.push(crate::model::ChildWorktree {
            path: "/tmp/wt/child".into(),
            branch: Some("child".into()),
            head: None,
            clean: None,
            ahead_behind: None,
            tmux_session: None,
            modified_at: None,
        });
        let mut repo = repo("/repo", vec![agent]);
        repo.pinned = true;
        repo.main_subagents_active = 2;

        let json = serde_json::to_string(&repo).unwrap();
        assert!(!json.contains("children"));
        assert!(!json.contains("subagents"));
        assert!(!json.contains("main_subagents_active"));
        let loaded: RepoNode = serde_json::from_str(&json).unwrap();
        assert!(loaded.pinned);
        assert_eq!(loaded.main_subagents_active, 0);
        assert!(loaded.agents[0].subagents.is_empty());
        assert!(loaded.agents[0].children.is_empty());

        let mut legacy = serde_json::to_value(&repo).unwrap();
        legacy.as_object_mut().unwrap().remove("pinned");
        let legacy: RepoNode = serde_json::from_value(legacy).unwrap();
        assert!(!legacy.pinned);
    }

    #[test]
    fn merge_carries_agents_into_rediscovered_repo() {
        let mut registry = Registry {
            version: 1,
            repos: vec![repo("/a/one", vec![dummy_agent()])],
        };
        registry.merge_discovered(vec![repo("/a/one", Vec::new())]);
        assert_eq!(registry.repos.len(), 1);
        assert_eq!(registry.repos[0].agents.len(), 1);
    }

    #[test]
    fn merge_carries_pinned_into_rediscovered_repo() {
        let mut pinned = repo("/a/one", Vec::new());
        pinned.pinned = true;
        let mut registry = Registry {
            version: 1,
            repos: vec![pinned],
        };

        registry.merge_discovered(vec![repo("/a/one", Vec::new())]);

        assert_eq!(registry.repos.len(), 1);
        assert!(registry.repos[0].pinned);
    }
}
