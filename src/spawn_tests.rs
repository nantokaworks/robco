use super::*;
use crate::git::test_repo::TestRepo;

fn session_env(vars: &[(&str, &str)]) -> SessionEnv {
    SessionEnv::from_config_vars(vars)
}

fn repo(path: &std::path::Path, name: &str) -> RepoNode {
    RepoNode {
        path: path.into(),
        name: name.into(),
        remote_url: None,
        pinned: false,
        management: crate::model::ManagementMode::Auto,
        agents: Vec::new(),
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
    }
}

#[test]
fn no_conflict_when_the_branch_is_free() {
    let fixture = TestRepo::new();
    let repo = repo(fixture.path(), "myapp");

    let conflict = branch_conflict_for(
        &repo,
        "Add a thing",
        Some("42-dropr-Add-a-thing"),
        &Config::default(),
    )
    .unwrap();

    assert_eq!(conflict, None);
}

#[test]
fn a_branch_left_by_a_previous_attempt_is_reported_by_name() {
    // The exact incident this guards: a worker's first attempt opened a pull
    // request and left its branch behind; the task stayed open in dropr and
    // came back around as a ready candidate. The second attempt must see the
    // branch before `git worktree add` ever runs, not learn about it from a
    // failed command (dropr:_ord_VtFSIiLgWpgmDAGm).
    let fixture = TestRepo::new();
    let config = Config::default();
    let repo = repo(fixture.path(), "myapp");
    let branch = agent::worker_branch_name(
        &config,
        "myapp",
        "Add a thing",
        Some("42-dropr-Add-a-thing"),
    );
    fixture.feature_branch(&branch, "feature.txt");

    let conflict =
        branch_conflict_for(&repo, "Add a thing", Some("42-dropr-Add-a-thing"), &config).unwrap();

    assert_eq!(conflict.as_deref(), Some(branch.as_str()));
}

#[test]
fn the_conflict_check_is_stable_across_repeated_passes() {
    // Nothing about the check consumes or mutates the branch, so a dispatch
    // pass that re-runs it every 60 seconds — as the daemon does while the
    // task stays a ready candidate — gets the same answer every time instead
    // of drifting into a real `git worktree add` attempt on a later pass.
    let fixture = TestRepo::new();
    let config = Config::default();
    let repo = repo(fixture.path(), "myapp");
    let branch = agent::worker_branch_name(
        &config,
        "myapp",
        "Add a thing",
        Some("42-dropr-Add-a-thing"),
    );
    fixture.feature_branch(&branch, "feature.txt");

    for _ in 0..3 {
        let conflict =
            branch_conflict_for(&repo, "Add a thing", Some("42-dropr-Add-a-thing"), &config)
                .unwrap();
        assert_eq!(conflict.as_deref(), Some(branch.as_str()));
    }
}

#[test]
fn configured_session_credential_survives_the_worker_blocklist() {
    let blocked = vec![
        ("CLAUDE_CODE_OAUTH_TOKEN".to_string(), String::new()),
        ("GITHUB_TOKEN".to_string(), String::new()),
    ];

    let env = worker_env(
        blocked,
        &session_env(&[("CLAUDE_CODE_OAUTH_TOKEN", "token")]),
    );

    assert_eq!(
        env,
        vec![
            ("GITHUB_TOKEN".to_string(), String::new()),
            ("CLAUDE_CODE_OAUTH_TOKEN".to_string(), "token".to_string()),
        ]
    );
}

#[test]
fn an_unconfigured_credential_is_still_blanked() {
    let blocked = vec![("AWS_SECRET".to_string(), String::new())];

    let env = worker_env(blocked, &session_env(&[]));

    assert_eq!(env, vec![("AWS_SECRET".to_string(), String::new())]);
}

#[test]
fn interactive_spawns_still_receive_the_session_channel() {
    // `blocked` is empty for a non-autonomous spawn; the channel is not.
    let env = worker_env(Vec::new(), &session_env(&[("ANTHROPIC_API_KEY", "key")]));

    assert_eq!(
        env,
        vec![("ANTHROPIC_API_KEY".to_string(), "key".to_string())]
    );
}

#[test]
fn outcome_copies_agent_shape() {
    let outcome = SpawnOutcome {
        id: "worker".into(),
        branch: "repo/task".into(),
        worktree_path: "/tmp/worktree".into(),
        tmux_session: "robco-task".into(),
    };
    let value = serde_json::to_value(outcome).unwrap();
    assert_eq!(value["id"], "worker");
    assert_eq!(value["branch"], "repo/task");
    assert_eq!(value["worktree_path"], "/tmp/worktree");
    assert_eq!(value["tmux_session"], "robco-task");
}
