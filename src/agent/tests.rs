use super::*;

#[test]
fn naming_slug_caps_at_word_boundary() {
    assert_eq!(
        cap_name_slug("task-145-this-is-a-very-long-task-title"),
        "task-145-this-is-a-very-long"
    );
}

#[test]
fn naming_slug_keeps_the_number_and_source_when_capping() {
    // The number is what must survive the cap. Capping cuts at the last hyphen
    // inside the budget, and the leading `<n>-dropr-` sits well inside it.
    assert_eq!(
        cap_name_slug("297-dropr-Lead-auto-created-worktree-branch-and-session-names"),
        "297-dropr-Lead-auto-created"
    );
    // A title carrying no hyphen of its own still cannot eat the prefix: the
    // hyphen after the source is the boundary the cap falls back to.
    assert_eq!(
        cap_name_slug("297-dropr-Leadautocreatedworktreebranchnames"),
        "297-dropr"
    );
}

#[test]
fn naming_slug_trims_trailing_hyphen() {
    assert_eq!(cap_name_slug("short-title-"), "short-title");
}

#[test]
fn naming_slug_leaves_short_value_unchanged() {
    assert_eq!(cap_name_slug("short-title"), "short-title");
}

#[test]
fn naming_slug_hard_truncates_without_hyphens() {
    assert_eq!(
        cap_name_slug("abcdefghijklmnopqrstuvwxyz0123456789"),
        "abcdefghijklmnopqrstuvwxyz012345"
    );
}

#[test]
fn naming_slug_leaves_exactly_32_characters_unchanged() {
    let slug = "abcdefghijklmnopqrstuvwxyz012345";
    assert_eq!(slug.chars().count(), 32);
    assert_eq!(cap_name_slug(slug), slug);
}

#[test]
fn naming_slug_caps_unicode_by_character_without_panicking() {
    let slug = "é".repeat(40);
    assert_eq!(cap_name_slug(&slug), "é".repeat(32));
}

#[test]
fn naming_slug_falls_back_for_empty_input() {
    assert_eq!(naming_slug("", None), "agent");
}

#[test]
fn naming_slug_falls_back_when_title_sanitizes_to_empty() {
    assert_eq!(naming_slug("✨", None), "agent");
}

#[test]
fn naming_slug_falls_back_when_explicit_slug_is_only_hyphens() {
    assert_eq!(naming_slug("ignored title", Some("---")), "agent");
}

#[test]
fn naming_slug_sanitizes_explicit_value() {
    assert_eq!(
        naming_slug("ignored title", Some("task 145/explicit")),
        "task-145-explicit"
    );
}

#[test]
fn naming_slug_prefers_explicit_value() {
    assert_eq!(
        naming_slug("ignored title", Some("task-145-explicit-name")),
        "task-145-explicit-name"
    );
}
use std::process::Command;

#[test]
fn quotes_initial_prompt_for_shell_command() {
    assert_eq!(
        launch_command("claude", Some("fix Bob's bug"), &[]),
        "claude 'fix Bob'\\''s bug'"
    );
}

#[test]
fn branch_prefix_defaults_to_repo_name() {
    let config = Config::default();
    assert_eq!(resolve_branch_prefix(&config, "myapp"), "myapp/");
}

#[test]
fn branch_prefix_uses_explicit_override() {
    let config = Config {
        branch_prefix: Some("robco/".to_string()),
        ..Config::default()
    };
    assert_eq!(resolve_branch_prefix(&config, "myapp"), "robco/");
}

#[test]
fn branch_prefix_sanitizes_repo_name() {
    let config = Config::default();
    assert_eq!(resolve_branch_prefix(&config, "my.repo"), "my-repo/");
}

#[test]
fn branch_prefix_falls_back_when_repo_name_sanitizes_to_empty() {
    let config = Config::default();
    assert_eq!(resolve_branch_prefix(&config, "..."), "robco/");
}

#[test]
fn worker_branch_name_matches_the_prefix_and_slug_it_is_built_from() {
    // The formula `crate::spawn::branch_conflict` checks with must never drift
    // from the one `create_agent_with_launch` actually spawns with — this pins
    // the two together.
    let config = Config::default();
    assert_eq!(
        worker_branch_name(
            &config,
            "myapp",
            "Add a thing",
            Some("42-dropr-Add-a-thing")
        ),
        format!(
            "{}{}",
            resolve_branch_prefix(&config, "myapp"),
            naming_slug("Add a thing", Some("42-dropr-Add-a-thing"))
        )
    );
}

fn repo_named(name: &str) -> RepoNode {
    RepoNode {
        path: format!("/tmp/{name}").into(),
        name: name.to_string(),
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
fn adopt_strips_branch_prefix_to_match_created_session_name() {
    let config = Config::default();
    let repo = repo_named("dropr");
    let adopted = adopt_worktree(
        &repo,
        &config,
        "/tmp/wt".into(),
        Some("dropr/support-open-claw".to_string()),
        None,
        None,
        None,
    );
    // Same session name create_agent(title = "support-open-claw") produces.
    assert_eq!(adopted.tmux_session, "robco_dropr_support-open-claw");
    assert_eq!(adopted.title, "support-open-claw");
    assert_eq!(adopted.branch, "dropr/support-open-claw");
}

#[test]
fn adopt_binds_to_existing_session_over_derived_name() {
    let config = Config::default();
    let repo = repo_named("dropr");
    let adopted = adopt_worktree(
        &repo,
        &config,
        "/tmp/wt".into(),
        Some("dropr/support-open-claw".to_string()),
        None,
        Some("robco_dropr_legacy-name".to_string()),
        None,
    );
    assert_eq!(adopted.tmux_session, "robco_dropr_legacy-name");
    // Title still derives from the branch — only the session binding differs.
    assert_eq!(adopted.title, "support-open-claw");
}

#[test]
fn adopt_preserves_recovered_identity() {
    let adopted = adopt_worktree(
        &repo_named("dropr"),
        &Config::default(),
        "/tmp/wt".into(),
        Some("dropr/child".into()),
        None,
        Some("robco_dropr_child".into()),
        Some(RecoveredIdentity {
            id: "child-id".into(),
            parent_agent_id: Some("parent-id".into()),
        }),
    );
    assert_eq!(adopted.id, "child-id");
    assert_eq!(adopted.parent_agent_id.as_deref(), Some("parent-id"));
    assert_eq!(adopted.management, crate::model::ManagementMode::Manual);
    assert!(adopted.claude_session_id.is_none());
}

/// A re-adopted Overseer worker must return under automatic dispatch: a
/// hardcoded `Manual` would leave it counted as manual and its task skipped.
#[test]
fn adopted_overseer_worker_comes_back_as_auto() {
    let adopted = adopt_worktree(
        &repo_named("dropr"),
        &Config::default(),
        "/tmp/wt".into(),
        Some("dropr/worker".into()),
        None,
        Some("robco_dropr_worker".into()),
        Some(RecoveredIdentity {
            id: "worker-id".into(),
            parent_agent_id: Some(crate::overseer::OVERSEER_AGENT_ID.into()),
        }),
    );
    assert_eq!(adopted.management, crate::model::ManagementMode::Auto);
}

/// A hand-made worktree has no session to recover a parent from, so it is
/// adopted unowned and stays `Manual` until `g` enrolls it.
#[test]
fn adopted_worktree_without_a_recovered_parent_stays_manual() {
    let adopted = adopt_worktree(
        &repo_named("dropr"),
        &Config::default(),
        "/tmp/wt".into(),
        Some("dropr/hand-made".into()),
        None,
        None,
        None,
    );
    assert_eq!(adopted.parent_agent_id, None);
    assert_eq!(adopted.management, crate::model::ManagementMode::Manual);
}

fn agent_titled(title: &str, branch: &str) -> AgentNode {
    let now = chrono::Local::now();
    AgentNode {
        management: crate::model::ManagementMode::Manual,
        id: "agent123".to_string(),
        parent_agent_id: None,
        title: title.to_string(),
        task_number: None,
        worktree_path: "/tmp/wt".into(),
        branch: branch.to_string(),
        base_commit: String::new(),
        program: "claude".to_string(),
        claude_session_id: None,
        profile: None,
        tmux_session: "robco_dropr_t".to_string(),
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
fn normalize_strips_prefix_from_legacy_adopted_title() {
    let config = Config::default();
    let mut repo = repo_named("dropr");
    repo.agents.push(agent_titled(
        "dropr/support-open-claw",
        "dropr/support-open-claw",
    ));
    let mut repos = vec![repo];
    assert!(normalize_adopted_titles(&mut repos, &config));
    assert_eq!(repos[0].agents[0].title, "support-open-claw");
    // The branch itself is untouched — only the display title migrates.
    assert_eq!(repos[0].agents[0].branch, "dropr/support-open-claw");
}

#[test]
fn normalize_keeps_user_typed_title() {
    let config = Config::default();
    let mut repo = repo_named("dropr");
    // create_agent stores the raw title, so title != branch for robco-created
    // agents even when the title happens to contain the repo name.
    repo.agents
        .push(agent_titled("dropr/cleanup", "dropr/dropr-cleanup"));
    let mut repos = vec![repo];
    assert!(!normalize_adopted_titles(&mut repos, &config));
    assert_eq!(repos[0].agents[0].title, "dropr/cleanup");
}

#[test]
fn normalize_keeps_foreign_branch_and_detached_labels() {
    let config = Config::default();
    let mut repo = repo_named("dropr");
    repo.agents.push(agent_titled("feature/x", "feature/x"));
    repo.agents.push(agent_titled("wt-dir", "(detached)"));
    let mut repos = vec![repo];
    assert!(!normalize_adopted_titles(&mut repos, &config));
    assert_eq!(repos[0].agents[0].title, "feature/x");
    assert_eq!(repos[0].agents[1].title, "wt-dir");
}

#[test]
fn adopt_keeps_full_label_for_foreign_branch() {
    let config = Config::default();
    let repo = repo_named("dropr");
    let adopted = adopt_worktree(
        &repo,
        &config,
        "/tmp/wt".into(),
        Some("feature/x".to_string()),
        None,
        None,
        None,
    );
    assert_eq!(adopted.tmux_session, "robco_dropr_feature-x");
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
    let agent = adopt_worktree(
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

fn run_git(cwd: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(["-C", cwd.to_str().unwrap()])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
