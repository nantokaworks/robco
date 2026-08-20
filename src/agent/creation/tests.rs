use super::*;
use crate::agent::env::RecoveredIdentity;
use crate::agent::test_support::{agent_titled, repo_named};
use crate::git::test_repo::TestRepo;

fn repo_at(repo: &TestRepo, name: &str) -> RepoNode {
    let mut node = repo_named(name);
    node.path = repo.path().to_path_buf();
    node
}

/// dropr:503 — a worker's base commit comes from the repository's own
/// default branch, not a hardcoded `main`.
#[test]
fn resolve_base_branch_follows_a_master_default_repository() {
    let repo = TestRepo::new_with_default_branch("master");
    assert_eq!(
        resolve_base_branch(&repo_at(&repo, "myapp")).unwrap(),
        "master"
    );
}

#[test]
fn resolve_base_branch_follows_the_default_main_fixture() {
    let repo = TestRepo::new();
    assert_eq!(
        resolve_base_branch(&repo_at(&repo, "myapp")).unwrap(),
        "main"
    );
}

/// A repository with no `origin` at all must error rather than guess `main`.
#[test]
fn resolve_base_branch_errors_when_origin_head_is_unresolved() {
    let temp = tempfile::tempdir().unwrap();
    crate::git::test_repo::git(temp.path(), &["init", "-q"]);
    let mut node = repo_named("myapp");
    node.path = temp.path().to_path_buf();

    assert!(resolve_base_branch(&node).is_err());
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
    assert!(adopted.claude_session_id.is_none());
}

/// A hand-made worktree has no session to recover a parent from, so it is
/// adopted unowned, with no parent at all.
#[test]
fn adopted_worktree_without_a_recovered_parent_stays_unowned() {
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

/// A worker created with no parent is enrolled with the Overseer.
#[test]
fn create_agent_with_no_parent_is_enrolled_with_overseer() {
    let parent_agent_id = enroll_with_overseer(None);
    assert_eq!(
        parent_agent_id.as_deref(),
        Some(crate::overseer::OVERSEER_AGENT_ID)
    );
}

/// A worker created with a supplied parent keeps exactly that parent.
#[test]
fn create_agent_with_supplied_parent_keeps_it() {
    let parent_agent_id = enroll_with_overseer(Some("some-other-agent"));
    assert_eq!(parent_agent_id.as_deref(), Some("some-other-agent"));
}

/// A subagent spawned by an Overseer-managed worker (its own id is the
/// Overseer's) keeps that parent unmodified.
#[test]
fn create_agent_with_overseer_as_supplied_parent_is_kept() {
    let parent_agent_id = enroll_with_overseer(Some(crate::overseer::OVERSEER_AGENT_ID));
    assert_eq!(
        parent_agent_id.as_deref(),
        Some(crate::overseer::OVERSEER_AGENT_ID)
    );
}

/// A worker enrolled with the Overseer can resolve a report target.
///
/// `create_agent_with_launch` feeds `enroll_with_overseer`'s output straight
/// into `agent_env`, which is what ends up as `ROBCO_PARENT_AGENT_ID` in the
/// worker's tmux session. `src/mcp/tools/report.rs` reads that same env var
/// name back to find a report target. This test walks the same path
/// (enrol -> build env) and checks the value it produces is one
/// `is_overseer_child` accepts, so a mismatch between the two sides cannot
/// regress silently.
#[test]
fn worker_enrolled_with_overseer_can_resolve_a_report_target() {
    let parent_agent_id = enroll_with_overseer(None);
    let env = agent_env("worker-id", parent_agent_id.as_deref());

    let parent_value = env
        .iter()
        .find(|(key, _)| *key == crate::config::ENV_PARENT_AGENT_ID)
        .map(|(_, value)| value.as_str());

    assert_eq!(parent_value, Some(crate::overseer::OVERSEER_AGENT_ID));
    assert!(crate::overseer::is_overseer_child(parent_value));
}
