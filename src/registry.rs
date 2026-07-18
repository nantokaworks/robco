use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
};

use fd_lock::RwLock;
use nanoid::nanoid;

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
        self.save_at(&path)
    }

    pub fn add_pinned(&mut self, path: &Path) -> Result<bool> {
        let path = path.canonicalize()?;
        Ok(self.add_canonical_pinned(path))
    }

    pub fn locked_add_pinned(path: &Path) -> Result<()> {
        let path = path.canonicalize()?;
        Self::locked_update(|registry| {
            registry.add_canonical_pinned(path);
        })
        .map(|_| ())
    }

    fn add_canonical_pinned(&mut self, path: PathBuf) -> bool {
        if let Some(repo) = self.repos.iter_mut().find(|repo| repo.path == path) {
            let changed = !repo.pinned;
            repo.pinned = true;
            return changed;
        }
        self.repos.push(crate::discover::repo_node(path, true));
        true
    }

    /// Serialize a registry read-modify-write transaction across processes.
    pub fn locked_update<F>(f: F) -> Result<Registry>
    where
        F: FnOnce(&mut Registry),
    {
        ensure_robco_dir()?;
        Self::locked_update_at(&state_path()?, f)
    }

    fn locked_update_at<F>(path: &Path, f: F) -> Result<Registry>
    where
        F: FnOnce(&mut Registry),
    {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Self::with_write_lock(path, || {
            let mut registry = if path.exists() {
                let raw = fs::read_to_string(path)?;
                serde_json::from_str(&raw)?
            } else {
                Registry {
                    version: 1,
                    repos: Vec::new(),
                }
            };
            f(&mut registry);
            Self::write_unlocked(&registry, path)?;
            Ok(registry)
        })
    }

    fn save_at(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Self::with_write_lock(path, || Self::write_unlocked(self, path))
    }

    fn with_write_lock<T>(path: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
        let lock_file = OpenOptions::new()
            .create(true)
            // Lock file contents are irrelevant; keep them untouched.
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.with_extension("json.lock"))?;
        let mut lock = RwLock::new(lock_file);
        let _guard = lock.write()?;
        f()
    }

    fn write_unlocked(registry: &Registry, path: &Path) -> Result<()> {
        let raw = serde_json::to_string_pretty(registry)?;
        let temp_path = path.with_extension(format!("json.{}.tmp", nanoid!()));
        let written = fs::write(&temp_path, raw).and_then(|()| fs::rename(&temp_path, path));
        if let Err(error) = written {
            let _ = fs::remove_file(temp_path);
            return Err(error.into());
        }
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
    use std::{sync::Arc, time::SystemTime};

    use super::*;
    use crate::model::{AgentNode, ManagementMode, RepoNode};
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
            parent_agent_id: None,
            management: ManagementMode::Manual,
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
    fn parent_agent_id_defaults_and_round_trips() {
        let mut agent = dummy_agent();
        let mut legacy = serde_json::to_value(&agent).unwrap();
        legacy.as_object_mut().unwrap().remove("parent_agent_id");
        let loaded: AgentNode = serde_json::from_value(legacy).unwrap();
        assert_eq!(loaded.parent_agent_id, None);

        agent.parent_agent_id = Some("parent123".into());
        let loaded: AgentNode =
            serde_json::from_str(&serde_json::to_string(&agent).unwrap()).unwrap();
        assert_eq!(loaded.parent_agent_id.as_deref(), Some("parent123"));
    }

    #[test]
    fn management_mode_round_trips() {
        let mut agent = dummy_agent();
        agent.management = ManagementMode::Auto;
        let loaded: AgentNode =
            serde_json::from_str(&serde_json::to_string(&agent).unwrap()).unwrap();
        assert_eq!(loaded.management, ManagementMode::Auto);

        agent.management = ManagementMode::Manual;
        let loaded: AgentNode =
            serde_json::from_str(&serde_json::to_string(&agent).unwrap()).unwrap();
        assert_eq!(loaded.management, ManagementMode::Manual);
    }

    #[test]
    fn missing_management_field_defaults_to_auto() {
        let registry = Registry {
            version: 1,
            repos: vec![repo("/repo", vec![dummy_agent()])],
        };
        let mut legacy = serde_json::to_value(&registry).unwrap();
        legacy["repos"][0]["agents"][0]
            .as_object_mut()
            .unwrap()
            .remove("management");
        let loaded: Registry = serde_json::from_value(legacy).unwrap();
        assert_eq!(loaded.repos[0].agents[0].management, ManagementMode::Auto);
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

    #[test]
    fn legacy_state_deserializes_without_overseer_fields() {
        let registry: Registry = serde_json::from_str(r#"{"version":1,"repos":[]}"#).unwrap();
        assert_eq!(registry.version, 1);
        assert!(registry.repos.is_empty());
    }

    #[test]
    fn locked_update_serializes_concurrent_writers() {
        let temp = tempfile::tempdir().unwrap();
        let path = Arc::new(temp.path().join("state.json"));
        let threads: Vec<_> = (0..2)
            .map(|_| {
                let path = Arc::clone(&path);
                std::thread::spawn(move || {
                    for _ in 0..25 {
                        Registry::locked_update_at(&path, |registry| registry.version += 1)
                            .unwrap();
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }

        let raw = fs::read_to_string(path.as_ref()).unwrap();
        let registry: Registry = serde_json::from_str(&raw).unwrap();
        assert_eq!(registry.version, 51);
    }

    #[test]
    fn save_under_lock_round_trips() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.json");
        let registry = Registry {
            version: 7,
            repos: vec![repo("/a/one", Vec::new())],
        };

        registry.save_at(&path).unwrap();

        let raw = fs::read_to_string(path).unwrap();
        let loaded: Registry = serde_json::from_str(&raw).unwrap();
        assert_eq!(loaded.version, 7);
        assert_eq!(loaded.repos.len(), 1);
    }
}
