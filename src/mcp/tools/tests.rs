use std::time::{Duration, SystemTime};

use chrono::Local;
use serde_json::json;

use crate::subagents::{SubagentStatus, TaskSubagent};

use super::*;

pub(super) fn registry_with_agent(id: &str) -> Registry {
    Registry {
        version: 1,
        repos: vec![RepoNode {
            path: "/repo".into(),
            name: "repo".to_string(),
            remote_url: None,
            pinned: false,
            agents: vec![AgentNode {
                id: id.to_string(),
                parent_agent_id: None,
                title: "task".to_string(),
                task_number: None,
                worktree_path: "/repo-wt".into(),
                branch: "task".to_string(),
                base_commit: "abc".to_string(),
                program: "codex".to_string(),
                spawned_by_version: None,
                claude_session_id: None,
                profile: None,
                tmux_session: "robco-task".to_string(),
                created_at: Local::now(),
                updated_at: Local::now(),
                status: Status::Idle,
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
            }],
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
        }],
    }
}

#[test]
fn parses_required_agent_id_args() {
    let args: AgentIdArgs = parse_args(Some(json!({ "agent_id": "a1" }))).unwrap();
    assert_eq!(args.agent_id, "a1");
    assert!(parse_args::<AgentIdArgs>(Some(json!({}))).is_err());
    assert!(validate_non_blank("agent_id", " ").is_err());
}

#[test]
fn parse_failures_are_invalid_params() {
    let err = match parse_args::<AgentIdArgs>(Some(json!({ "agent_id": 7 }))) {
        Ok(_) => panic!("expected invalid params"),
        Err(err) => err,
    };
    assert!(matches!(err, ToolError::InvalidParams(_)));
}

#[test]
fn finds_agent_by_id() {
    let registry = registry_with_agent("a1");
    let (_, agent) = find_agent(&registry, "a1").unwrap();
    assert_eq!(agent.tmux_session, "robco-task");
    let err = find_agent(&registry, "missing").unwrap_err();
    assert!(matches!(err, ToolError::Execution(_)));
}

#[test]
fn tails_last_non_empty_lines() {
    let text = "a\n\nb\nc\n";
    assert_eq!(tail_non_empty_lines(text, 2), "b\nc");
}

#[test]
fn dead_agent_tools_suppress_cached_subagent_activity() {
    let mut registry = registry_with_agent("a1");
    let agent = &mut registry.repos[0].agents[0];
    agent.tracked_command = Some("cargo".into());
    agent.subagents = vec![
        subagent("running", SubagentStatus::Running),
        subagent("done", SubagentStatus::Done),
    ];

    let listed = agent_list(&registry).unwrap();
    let status = agent_status(&registry, "a1").unwrap();
    assert_eq!(listed["repos"][0]["agents"][0]["tracked_command"], "cargo");
    assert_eq!(listed["repos"][0]["agents"][0]["status"], "dead");
    assert_eq!(listed["repos"][0]["agents"][0]["worktree_missing"], false);
    assert_eq!(listed["repos"][0]["agents"][0]["subagents_active"], 0);
    assert_eq!(status["tracked_command"], "cargo");
    assert_eq!(status["status"], "dead");
    assert_eq!(status["worktree_missing"], false);
    assert_eq!(status["subagents_active"], 0);
}

#[test]
fn live_agent_activity_counts_running_subagents() {
    let temp = tempfile::tempdir().unwrap();
    let mut registry = registry_with_agent("a1");
    let agent = &mut registry.repos[0].agents[0];
    agent.worktree_path = temp.path().into();
    agent.subagents = vec![
        subagent("running", SubagentStatus::Running),
        subagent("done", SubagentStatus::Done),
    ];

    assert_eq!(live_activity(agent, Status::Running).1, 1);
}

#[test]
fn activity_output_schemas_require_new_fields() {
    let tools = catalog::list_tools();
    for name in ["robco_agent_list", "robco_agent_status"] {
        let tool = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap();
        let schema = &tool["outputSchema"];
        assert!(schema.to_string().contains("tracked_command"));
        assert!(schema.to_string().contains("subagents_active"));
        assert!(schema.to_string().contains("worktree_missing"));
    }
}

#[test]
fn catalog_includes_agent_create_and_every_tool() {
    let tools = catalog::list_tools();
    let tools = tools.as_array().unwrap();
    assert_eq!(tools.len(), 18);
    let create = tools
        .iter()
        .find(|tool| tool["name"] == "robco_agent_create")
        .unwrap();
    assert_eq!(create["inputSchema"]["required"], json!(["repo"]));
    assert!(
        create["outputSchema"]["properties"]
            .get("worktree_path")
            .is_some()
    );
}

fn subagent(id: &str, status: SubagentStatus) -> TaskSubagent {
    TaskSubagent {
        id: id.into(),
        agent_type: "Explore".into(),
        description: "inspect".into(),
        spawn_depth: 1,
        started_at: SystemTime::now() - Duration::from_secs(10),
        last_activity_at: SystemTime::now(),
        status,
    }
}

#[test]
fn whoami_resolves_known_agent_and_parent() {
    let registry = registry_with_agent("a1");
    let result = identity::whoami_with_lookup(
        |key| match key {
            crate::config::ENV_AGENT_ID => Some("a1".to_string()),
            crate::config::ENV_PARENT_AGENT_ID => Some("parent-1".to_string()),
            _ => None,
        },
        || Ok(registry),
    )
    .unwrap();

    assert_eq!(
        result,
        json!({
            "agent_id": "a1",
            "title": "task",
            "repo": "repo",
            "parent_agent_id": "parent-1"
        })
    );
}

#[test]
fn whoami_returns_unknown_raw_agent_id() {
    let registry = registry_with_agent("a1");
    let result = identity::whoami_with_lookup(
        |key| (key == crate::config::ENV_AGENT_ID).then(|| "missing".to_string()),
        || Ok(registry),
    )
    .unwrap();

    assert_eq!(
        result,
        json!({
            "agent_id": "missing",
            "title": null,
            "repo": null,
            "parent_agent_id": null
        })
    );
}

#[test]
fn whoami_treats_empty_identity_values_as_unset() {
    let result = identity::whoami_with_lookup(
        |_| Some(String::new()),
        || panic!("empty identity must not load the registry"),
    )
    .unwrap();

    assert_eq!(
        result,
        json!({
            "agent_id": null,
            "title": null,
            "repo": null,
            "parent_agent_id": null
        })
    );
}

#[test]
fn legacy_chief_policy_name_is_dispatched_as_an_alias() {
    let error = call_tool("robco_chief_policy", Some(json!({ "unexpected": true }))).unwrap_err();
    assert!(!error.to_string().contains("unknown tool"));
}

#[test]
fn overseer_policy_includes_daemon_health() {
    let policy = call_tool("robco_overseer_policy", Some(json!({}))).unwrap();
    assert!(policy.get("daemon_alive").is_some());
    assert!(policy.get("auto_merge").is_some());
}
