use chrono::Local;
use serde_json::json;

use super::*;

pub(super) fn registry_with_agent(id: &str) -> Registry {
    Registry {
        version: 1,
        repos: vec![RepoNode {
            path: "/repo".into(),
            name: "repo".to_string(),
            remote_url: None,
            agents: vec![AgentNode {
                id: id.to_string(),
                title: "task".to_string(),
                worktree_path: "/repo-wt".into(),
                branch: "task".to_string(),
                base_commit: "abc".to_string(),
                program: "codex".to_string(),
                profile: None,
                tmux_session: "robco-task".to_string(),
                created_at: Local::now(),
                updated_at: Local::now(),
                status: Status::Idle,
                last_capture: None,
                last_change_at: None,
                last_auto_accept_at: None,
                shell_working: false,
                pane_pid: None,
                tracked_command: None,
                children: Vec::new(),
            }],
            dropr: None,
            main_status: None,
            main_last_capture: None,
            main_last_change_at: None,
            main_shell_working: false,
            main_pane_pid: None,
            main_tracked_command: None,
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
