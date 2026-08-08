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
        management: crate::model::ManagementMode::Auto,
        agents,
        dropr: None,
        dropr_tasks: crate::dropr::DroprTaskFetch::default(),
        main_status: None,
        main_last_capture: None,
        main_last_spinner: None,
        main_last_change_at: None,
        main_shell_working: false,
        main_mcp_active: false,
        main_pane_pid: None,
        main_tracked_command: None,
        main_subagents_active: 0,
        main_behind_origin: None,
        checkout_state: None,
    }
}

fn dummy_agent() -> AgentNode {
    let now = chrono::Local::now();
    AgentNode {
        id: "agent123".to_string(),
        parent_agent_id: None,
        management: ManagementMode::Manual,
        title: "t".to_string(),
        task_number: None,
        worktree_path: "/tmp/wt".into(),
        branch: "b".to_string(),
        base_commit: String::new(),
        program: "claude".to_string(),
        claude_session_id: None,
        profile: None,
        tmux_session: "robco_r_t".to_string(),
        created_at: now,
        updated_at: now,
        status: Default::default(),
        worktree_missing: false,
        merge_error: None,
        last_capture: None,
        last_spinner: None,
        last_change_at: None,
        last_auto_accept_at: None,
        shell_working: false,
        mcp_active: false,
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
    let loaded: AgentNode = serde_json::from_str(&serde_json::to_string(&agent).unwrap()).unwrap();
    assert_eq!(loaded.parent_agent_id.as_deref(), Some("parent123"));
}

#[test]
fn management_mode_round_trips() {
    let mut agent = dummy_agent();
    agent.management = ManagementMode::Auto;
    let loaded: AgentNode = serde_json::from_str(&serde_json::to_string(&agent).unwrap()).unwrap();
    assert_eq!(loaded.management, ManagementMode::Auto);

    agent.management = ManagementMode::Manual;
    let loaded: AgentNode = serde_json::from_str(&serde_json::to_string(&agent).unwrap()).unwrap();
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
                    Registry::locked_update_at(&path, |registry| registry.version += 1).unwrap();
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

/// The reason the UI's writes go through `locked_update`: a mutation closure
/// is applied to whatever is on disk, so a row another process committed in
/// the meantime survives — where writing a snapshot back erases it. Both
/// halves run against the same file so the difference is the write mode, not
/// the fixture.
#[test]
fn locked_update_keeps_a_row_a_stale_snapshot_would_erase() {
    fn paths(registry: &Registry) -> Vec<String> {
        registry
            .repos
            .iter()
            .map(|repo| repo.path.to_string_lossy().to_string())
            .collect()
    }

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.json");
    // What this process read at startup, before anyone else wrote.
    let snapshot = Registry {
        version: 1,
        repos: vec![repo("/a/one", Vec::new())],
    };
    snapshot.save_at(&path).unwrap();
    // Another writer — the Overseer daemon, the CLI — registers a repo.
    Registry::locked_update_at(&path, |registry| {
        registry.repos.push(repo("/b/two", Vec::new()));
    })
    .unwrap();

    // Dropping `/a/one` under the lock leaves the newcomer in place.
    let after = Registry::locked_update_at(&path, |registry| {
        registry
            .repos
            .retain(|stored| stored.path.to_string_lossy() != "/a/one");
    })
    .unwrap();
    assert_eq!(paths(&after), ["/b/two"]);

    // Writing the stale snapshot back instead would have erased it.
    snapshot.save_at(&path).unwrap();
    let clobbered: Registry = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(paths(&clobbered), ["/a/one"]);
}

#[test]
fn locked_load_reads_what_the_last_transaction_committed() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.json");

    // No state file yet is not an error; a reader that treated it as one would
    // have no row set to reconcile against.
    let empty = Registry::locked_load_at(&path).unwrap();
    assert_eq!(empty.version, 1);
    assert!(empty.repos.is_empty());

    Registry::locked_update_at(&path, |registry| {
        registry.repos.push(repo("/a/one", vec![dummy_agent()]));
    })
    .unwrap();

    let loaded = Registry::locked_load_at(&path).unwrap();
    assert_eq!(loaded.repos.len(), 1);
    assert_eq!(loaded.repos[0].agents[0].id, "agent123");
}

/// The shared lock must not exclude another reader: the UI's discovery pass
/// takes it on every tick, and self-deadlocking there would freeze the refresh.
#[test]
fn locked_load_admits_concurrent_readers() {
    let temp = tempfile::tempdir().unwrap();
    let path = Arc::new(temp.path().join("state.json"));
    Registry::locked_update_at(&path, |registry| registry.version = 9).unwrap();

    let threads: Vec<_> = (0..4)
        .map(|_| {
            let path = Arc::clone(&path);
            std::thread::spawn(move || {
                for _ in 0..25 {
                    assert_eq!(Registry::locked_load_at(&path).unwrap().version, 9);
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
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

#[test]
fn repo_label_prefers_the_registered_name_over_the_path() {
    let mut node = repo("/Users/operator/repos/robco", Vec::new());
    node.name = "robco".into();
    let registry = Registry {
        version: 1,
        repos: vec![node],
    };
    assert_eq!(registry.repo_label("/Users/operator/repos/robco"), "robco");
}

#[test]
fn repo_label_falls_back_to_the_last_path_component_never_the_full_path() {
    let registry = Registry {
        version: 1,
        repos: Vec::new(),
    };
    let label = registry.repo_label("/Users/operator/repos/orphaned-entry");
    assert_eq!(label, "orphaned-entry");
    assert!(!label.contains('/'));
}
