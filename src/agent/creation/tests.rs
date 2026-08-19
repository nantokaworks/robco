use super::*;
use crate::agent::env::RecoveredIdentity;
use crate::agent::test_support::{agent_titled, repo_named};

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
    let (parent_agent_id, management) = enroll_with_overseer(None);
    assert_eq!(
        parent_agent_id.as_deref(),
        Some(crate::overseer::OVERSEER_AGENT_ID)
    );
    assert_eq!(management, crate::model::ManagementMode::Auto);
}

/// A worker created with a supplied parent keeps exactly that parent.
#[test]
fn create_agent_with_supplied_parent_keeps_it() {
    let (parent_agent_id, management) = enroll_with_overseer(Some("some-other-agent"));
    assert_eq!(parent_agent_id.as_deref(), Some("some-other-agent"));
    assert_eq!(management, crate::model::ManagementMode::Manual);
}

/// A subagent spawned by an Overseer-managed worker (its own id is the
/// Overseer's) keeps that parent unmodified, and still reads as `Auto`.
#[test]
fn create_agent_with_overseer_as_supplied_parent_stays_auto() {
    let (parent_agent_id, management) =
        enroll_with_overseer(Some(crate::overseer::OVERSEER_AGENT_ID));
    assert_eq!(
        parent_agent_id.as_deref(),
        Some(crate::overseer::OVERSEER_AGENT_ID)
    );
    assert_eq!(management, crate::model::ManagementMode::Auto);
}
