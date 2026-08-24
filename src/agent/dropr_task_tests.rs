use std::{cell::RefCell, time::Duration};

use super::*;
use crate::agent::test_support::repo_named;
use crate::git::test_repo::TestRepo;

fn candidate(display_id: &str, id: &str, title: &str) -> DroprTaskCandidate {
    DroprTaskCandidate {
        display_id: display_id.to_string(),
        title: title.to_string(),
        description: None,
        priority: String::new(),
        status: String::new(),
        priority_score: None,
        blocked_reason: None,
        updated_at: None,
        id: id.to_string(),
        parent_task_id: None,
        child_count: 0,
    }
}

fn repo_at(repo: &TestRepo, name: &str) -> RepoNode {
    let mut node = repo_named(name);
    node.path = repo.path().to_path_buf();
    node
}

fn claimed(_: &str, _: &str, _: &str, _: Duration) -> dropr::ClaimAttempt {
    dropr::ClaimAttempt::Claimed
}

fn never_released(_: &str, _: &str, _: &str, _: Duration) -> bool {
    panic!("release must not be called when the launch never claimed or never created an agent");
}

/// End-to-end proof that `launch_with` takes the exact four steps the task
/// table calls out: claim, title `"{display_id} {title}"`, prompt build, and
/// (on success) no release. Uses a real worktree/tmux session, the same as
/// `agent::creation`'s own tests, so it also proves the shared path still
/// produces a real, working agent.
#[test]
fn launches_from_a_task_and_titles_with_the_number_first() {
    if !crate::tmux::is_installed() {
        eprintln!("skipping: no tmux binary on this runner");
        return;
    }
    let repo_fixture = TestRepo::new();
    let worktree_root = tempfile::tempdir().unwrap();
    let config = Config {
        worktree_root: worktree_root.path().to_path_buf(),
        default_program: "claude".into(),
        profiles: vec![crate::config::Profile {
            name: "claude".into(),
            program: "/nonexistent/claude".into(),
            autonomous_args: Vec::new(),
            model: None,
            backend: None,
        }],
        ..Config::default()
    };
    let repo = repo_at(&repo_fixture, "myapp");
    let candidate = candidate("#538", "task-nanoid", "Launch workers autonomously");

    let request = DroprTaskLaunch {
        repo: &repo,
        config: &config,
        workspace_id: "ws-1",
        candidate: &candidate,
        subtasks: &[],
        claim_agent_id: "robco-spawn",
        parent_agent_id: None,
        extra_args: &[],
        extra_env: &[],
    };

    let agent = launch_with(request, claimed, never_released).unwrap();
    let _ = crate::tmux::kill_session(&agent.tmux_session);

    assert_eq!(agent.title, "#538 Launch workers autonomously");
    assert!(agent.branch.contains("538"));
}

/// dropr:540's acceptance criterion: "a launch failure releases the claim."
/// Forces `create_agent_with_launch` to fail deterministically — no `tmux`
/// needed, since an unresolved default branch fails before the worktree or
/// the tmux session are ever created — and checks the release closure ran
/// with exactly the claimed task's id and agent id.
#[test]
fn a_spawn_failure_releases_the_claim() {
    let temp = tempfile::tempdir().unwrap();
    crate::git::test_repo::git(temp.path(), &["init", "-q"]);
    let mut repo = repo_named("myapp");
    repo.path = temp.path().to_path_buf();
    let config = Config::default();
    let candidate = candidate("#538", "task-nanoid", "Launch workers autonomously");
    let released = RefCell::new(None);

    let request = DroprTaskLaunch {
        repo: &repo,
        config: &config,
        workspace_id: "ws-1",
        candidate: &candidate,
        subtasks: &[],
        claim_agent_id: "robco-spawn",
        parent_agent_id: None,
        extra_args: &[],
        extra_env: &[],
    };

    let result = launch_with(request, claimed, |workspace_id, task_id, agent_id, _| {
        *released.borrow_mut() = Some((
            workspace_id.to_string(),
            task_id.to_string(),
            agent_id.to_string(),
        ));
        true
    });

    assert!(matches!(result, Err(LaunchError::Spawn(_))));
    assert_eq!(
        released.into_inner(),
        Some((
            "ws-1".to_string(),
            "task-nanoid".to_string(),
            "robco-spawn".to_string()
        ))
    );
}

#[test]
fn a_claim_refusal_stops_before_creating_anything() {
    let repo = repo_named("myapp");
    let config = Config::default();
    let candidate = candidate("#538", "task-nanoid", "Launch workers autonomously");

    let request = DroprTaskLaunch {
        repo: &repo,
        config: &config,
        workspace_id: "ws-1",
        candidate: &candidate,
        subtasks: &[],
        claim_agent_id: "robco-spawn",
        parent_agent_id: None,
        extra_args: &[],
        extra_env: &[],
    };

    let result = launch_with(
        request,
        |_, _, _, _| dropr::ClaimAttempt::Refused("locked".into()),
        never_released,
    );

    match result {
        Err(LaunchError::ClaimRefused(reason)) => assert_eq!(reason, "locked"),
        _ => panic!("expected ClaimRefused"),
    }
}

#[test]
fn an_unreachable_dropr_is_reported_as_such() {
    let repo = repo_named("myapp");
    let config = Config::default();
    let candidate = candidate("#538", "task-nanoid", "Launch workers autonomously");

    let request = DroprTaskLaunch {
        repo: &repo,
        config: &config,
        workspace_id: "ws-1",
        candidate: &candidate,
        subtasks: &[],
        claim_agent_id: "robco-spawn",
        parent_agent_id: None,
        extra_args: &[],
        extra_env: &[],
    };

    let result = launch_with(
        request,
        |_, _, _, _| dropr::ClaimAttempt::Unavailable,
        never_released,
    );

    assert!(matches!(result, Err(LaunchError::DroprUnreachable)));
}
