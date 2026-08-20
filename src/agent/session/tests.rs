use super::*;
use crate::agent::test_support::{agent_titled, repo_named, run_git};
use crate::config::Profile;

#[test]
fn repo_claude_command_appends_profile_autonomous_args() {
    let config = Config::default();
    assert_eq!(
        repo_claude_command(&config),
        "claude '--dangerously-skip-permissions'"
    );
}

#[test]
fn repo_claude_command_is_bare_program_without_autonomous_args() {
    let config = Config {
        profiles: vec![Profile {
            name: "claude".to_string(),
            program: "claude".to_string(),
            autonomous_args: Vec::new(),
            model: None,
            backend: None,
        }],
        ..Config::default()
    };
    assert_eq!(repo_claude_command(&config), "claude");
}

#[test]
fn relaunch_command_reuses_stored_claude_session_id() {
    let mut agent = agent_titled("title", "branch");
    agent.claude_session_id = Some("existing-id".to_string());
    assert_eq!(
        relaunch_command(&agent),
        "claude '--session-id' 'existing-id'"
    );

    agent.claude_session_id = None;
    assert_eq!(relaunch_command(&agent), "claude");
}

#[test]
fn kill_agent_rejects_registered_nested_worktree() {
    let temp = tempfile::tempdir().unwrap();
    let repo_path = temp.path().join("repo");
    run_git(temp.path(), &["init", repo_path.to_str().unwrap()]);
    run_git(&repo_path, &["config", "user.email", "robco@example.com"]);
    run_git(&repo_path, &["config", "user.name", "Robco Test"]);
    std::fs::write(repo_path.join("README"), "test\n").unwrap();
    run_git(&repo_path, &["add", "README"]);
    run_git(&repo_path, &["commit", "-m", "initial"]);

    let agent_path = temp.path().join("worktrees/agent");
    run_git(
        &repo_path,
        &[
            "worktree",
            "add",
            "-b",
            "agent",
            agent_path.to_str().unwrap(),
        ],
    );
    let child_path = agent_path.join("child");
    run_git(
        &repo_path,
        &[
            "worktree",
            "add",
            "-b",
            "child",
            child_path.to_str().unwrap(),
        ],
    );

    let mut repo = repo_named("repo");
    repo.path = repo_path;
    let config = Config::default();
    let agent = crate::agent::adopt_worktree(
        &repo,
        &config,
        agent_path.clone(),
        Some("agent".into()),
        None,
        None,
        None,
    );

    assert!(matches!(
        kill_agent(&repo, &agent, false),
        Err(crate::Error::ChildWorktreesPresent(path)) if path == agent_path
    ));
}
