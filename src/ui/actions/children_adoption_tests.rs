use super::test_support::{Fixture, add_worktree};
use super::*;

#[test]
fn adoption_grace_period_has_a_strict_boundary() {
    assert!(should_skip_adoption(std::time::Duration::from_secs(14)));
    assert!(!should_skip_adoption(std::time::Duration::from_secs(15)));
}

/// dropr:566 — a launching worker's tmux session comes up before
/// `spawn::persist_child` writes the registry row, so a refresh pass can run
/// in between and see a fresh worktree with a live session already attached.
/// Gating adoption on `session.is_none()` used to let that session short
/// circuit the grace period; only the worktree's own age should decide.
#[test]
fn fresh_sibling_with_a_live_session_still_waits_out_the_grace_period() {
    if !crate::tmux::is_installed() {
        eprintln!("skipping: no tmux binary on this runner");
        return;
    }
    let mut fixture = Fixture::new();
    fixture.config.tmux_server = crate::tmux::TmuxServer::for_tests();
    let sibling = fixture.config.worktree_root.join("sibling");
    add_worktree(&fixture.repo.path, &sibling, "sibling");

    let session = format!("{}sibling", fixture.config.tmux_session_prefix);
    crate::tmux::new_session(
        &fixture.config.tmux_server,
        &session,
        &sibling,
        "sleep 5",
        &[],
    )
    .unwrap();

    let (agent_added, _) = fixture.reconcile();

    let _ = crate::tmux::kill_session(&fixture.config.tmux_server, &session);

    assert!(!agent_added);
    assert_eq!(fixture.repo.agents.len(), 1);
}

#[test]
fn recovered_identity_of_a_tracked_agent_does_not_add_a_second_row() {
    let mut fixture = Fixture::new();
    let tracked_id = fixture.repo.agents[0].id.clone();
    let tracked_path = fixture.repo.agents[0].worktree_path.clone();
    // The worktree git reports is the tracked agent under a spelling `path_key`
    // cannot match — the registry entry's directory no longer canonicalizes, so
    // its lexical path is compared against git's. Its session still names the
    // tracked agent, which is what a second row would clone the id from.
    let renamed = fixture.config.worktree_root.join("dropr_task-749_renamed");
    let worktree = Worktree {
        path: renamed,
        head: None,
        branch: Some("dropr/task-749".into()),
    };

    let added = adopt_top_level(
        &mut fixture.repo,
        &fixture.config,
        worktree,
        Some("robco_repo_task-749".into()),
        Some(agent::RecoveredIdentity {
            id: tracked_id.clone(),
            parent_agent_id: None,
        }),
    );

    assert!(!added);
    assert_eq!(fixture.repo.agents.len(), 1);
    assert_eq!(fixture.repo.agents[0].id, tracked_id);
    assert_eq!(fixture.repo.agents[0].worktree_path, tracked_path);
}
