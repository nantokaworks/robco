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
        // discovered set. Keep the ones that still track agents — their
        // worktrees and tmux sessions are machine-global, so a launch
        // elsewhere must not erase the records needed to reattach them.
        // Agent-less leftovers carry no state worth keeping and are dropped.
        self.repos
            .extend(known.into_values().filter(|repo| !repo.agents.is_empty()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AgentNode, RepoNode};

    fn repo(path: &str, agents: Vec<AgentNode>) -> RepoNode {
        RepoNode {
            path: path.into(),
            name: path.rsplit('/').next().unwrap_or("repo").to_string(),
            remote_url: None,
            agents,
            dropr: None,
            main_status: None,
            main_last_capture: None,
            main_last_change_at: None,
            main_shell_working: false,
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
    fn merge_carries_agents_into_rediscovered_repo() {
        let mut registry = Registry {
            version: 1,
            repos: vec![repo("/a/one", vec![dummy_agent()])],
        };
        registry.merge_discovered(vec![repo("/a/one", Vec::new())]);
        assert_eq!(registry.repos.len(), 1);
        assert_eq!(registry.repos[0].agents.len(), 1);
    }
}
