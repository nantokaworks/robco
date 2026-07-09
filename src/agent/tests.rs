use super::*;

#[test]
fn quotes_initial_prompt_for_shell_command() {
    assert_eq!(
        launch_command("claude", Some("fix Bob's bug")),
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

fn repo_named(name: &str) -> RepoNode {
    RepoNode {
        path: format!("/tmp/{name}").into(),
        name: name.to_string(),
        remote_url: None,
        agents: Vec::new(),
        dropr: None,
        main_status: None,
        main_last_capture: None,
        main_last_change_at: None,
        main_shell_working: false,
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
    );
    assert_eq!(adopted.tmux_session, "robco_dropr_legacy-name");
    // Title still derives from the branch — only the session binding differs.
    assert_eq!(adopted.title, "support-open-claw");
}

fn agent_titled(title: &str, branch: &str) -> AgentNode {
    let now = chrono::Local::now();
    AgentNode {
        id: "agent123".to_string(),
        title: title.to_string(),
        worktree_path: "/tmp/wt".into(),
        branch: branch.to_string(),
        base_commit: String::new(),
        program: "claude".to_string(),
        profile: None,
        tmux_session: "robco_dropr_t".to_string(),
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
    );
    assert_eq!(adopted.tmux_session, "robco_dropr_feature-x");
}
