use super::*;
use crate::agent::test_support::fake_claude_binary;
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
        checkout_state: None,
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

/// dropr:532 — `robco spawn`'s own path (`spawn_in_repo_with_mode`, the third
/// of the three creation paths) still gets report hooks after hook
/// installation moved out of this file's own `prepare_worktree` closure and
/// into `create_agent_with_launch`. Run with `autonomous: false` on purpose:
/// hook installation is no longer gated on the autonomous flag (see the
/// dropr:532 decision scribble), only env-var blocklisting still is.
#[test]
fn spawn_in_repo_installs_report_hooks_even_when_not_autonomous() {
    use crate::registry::Registry;

    if !crate::tmux::is_installed() {
        eprintln!("skipping: no tmux binary on this runner (GitHub's macos-latest lacks one)");
        return;
    }

    let fixture = TestRepo::new();
    let worktree_root = tempfile::tempdir().unwrap();
    let bin_dir = tempfile::tempdir().unwrap();
    let config = Config {
        worktree_root: worktree_root.path().to_path_buf(),
        default_program: "claude".into(),
        profiles: vec![crate::config::Profile {
            name: "claude".into(),
            // A throwaway binary that just sleeps, so the launch verification
            // (dropr:554) sees a pane that actually stays up — it never
            // touches the real `claude` CLI either way.
            program: fake_claude_binary(bin_dir.path())
                .to_string_lossy()
                .into_owned(),
            autonomous_args: Vec::new(),
            model: None,
            backend: None,
            clear_command: None,
        }],
        tmux_server: crate::tmux::TmuxServer::for_tests(),
        ..Config::default()
    };
    // A name distinctive to this test, not `resolve_repo`'s exact-match
    // fixture name ("myapp") elsewhere in this file: `spawn_in_repo_with_mode`
    // reads the real (sandboxed, per-test-binary) `Registry::load()`, which is
    // shared with every other test in this process, so an unremarkable name
    // risks colliding with a row another concurrent test pushed.
    let repo_name = "dropr532-spawn-hook-check";
    // Additive, like `ui::actions::rename_tests`: the shared sandboxed
    // state.json may already carry rows from other tests running
    // concurrently in this process, so push rather than replace.
    Registry::locked_update(|registry| {
        registry.repos.push(repo(fixture.path(), repo_name));
    })
    .unwrap();

    let outcome = spawn_in_repo_with_mode(
        repo_name,
        "spawn hook check",
        None,
        None,
        None,
        &[],
        false,
        &config,
    )
    .unwrap();
    let _ = crate::tmux::kill_session(&config.tmux_server, &outcome.tmux_session);

    let settings: serde_json::Value = serde_json::from_slice(
        &std::fs::read(outcome.worktree_path.join(".claude/settings.local.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        settings["hooks"]["Stop"][0]["hooks"][0]["command"],
        "robco report --kind turn-done"
    );
    assert_eq!(
        settings["hooks"]["Notification"][0]["hooks"][0]["command"],
        "robco report --kind waiting"
    );
}

/// dropr:566 — a background refresh's `reconcile` can adopt a launching
/// worker's worktree before this call runs, leaving a row with the same id
/// already in the registry. `persist_child` must replace that row instead of
/// appending a second one that never gets the launch's own fields.
#[test]
fn persist_child_replaces_an_existing_row_with_the_same_id_instead_of_appending() {
    use crate::registry::Registry;

    let fixture = TestRepo::new();
    let repo_name = "dropr566-persist-child-upsert";
    let repo = repo(fixture.path(), repo_name);
    Registry::locked_update(|registry| {
        registry.repos.push(repo.clone());
    })
    .unwrap();

    let shared_id = "shared-id-566".to_string();
    let worktree_path = fixture.path().join("worktree");

    let adopted = agent::adopt_worktree(
        &repo,
        &Config::default(),
        worktree_path.clone(),
        Some("dropr/task-566".into()),
        None,
        None,
        Some(agent::RecoveredIdentity {
            id: shared_id.clone(),
            parent_agent_id: None,
        }),
    );
    let adopted_outcome = SpawnOutcome::from(&adopted);
    persist_child(&repo.path, adopted, &adopted_outcome).unwrap();

    let mut launched = agent::adopt_worktree(
        &repo,
        &Config::default(),
        worktree_path,
        Some("dropr/task-566".into()),
        None,
        None,
        Some(agent::RecoveredIdentity {
            id: shared_id.clone(),
            parent_agent_id: None,
        }),
    );
    launched.title = "#566 real title".into();
    launched.spawned_by_version = Some("0.7.0".into());
    launched.claude_session_id = Some("session-abc".into());
    let launched_outcome = SpawnOutcome::from(&launched);
    persist_child(&repo.path, launched, &launched_outcome).unwrap();

    let registry = Registry::load().unwrap();
    let persisted = registry
        .repos
        .iter()
        .find(|candidate| candidate.path == repo.path)
        .unwrap();
    let rows: Vec<_> = persisted
        .agents
        .iter()
        .filter(|agent| agent.id == shared_id)
        .collect();
    assert_eq!(rows.len(), 1, "expected exactly one row for the shared id");
    assert_eq!(rows[0].title, "#566 real title");
    assert_eq!(rows[0].spawned_by_version.as_deref(), Some("0.7.0"));
    assert_eq!(rows[0].claude_session_id.as_deref(), Some("session-abc"));
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
