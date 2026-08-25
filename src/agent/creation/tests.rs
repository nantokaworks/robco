use super::*;
use crate::agent::test_support::{fake_claude_binary, repo_named};
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

/// End-to-end proof that a worker created through the public `create_agent`
/// wrapper — the function both `src/ui/input.rs` (the `n` key) and
/// `src/ui/actions/dropr_task_worker.rs` (the dropr-task `n` key) call — gets
/// its report hooks written into the new worktree. Before dropr:532 these two
/// paths installed nothing at all, because hook installation lived behind a
/// per-caller closure only `spawn.rs` supplied; `create_agent_with_launch`
/// now installs them itself, so this single test covers both TUI paths at
/// once (they are, after this change, the exact same code).
#[test]
fn create_agent_installs_report_hooks_in_the_new_worktree() {
    if !crate::tmux::is_installed() {
        eprintln!("skipping: no tmux binary on this runner (GitHub's macos-latest lacks one)");
        return;
    }
    let repo_fixture = TestRepo::new();
    let worktree_root = tempfile::tempdir().unwrap();
    let bin_dir = tempfile::tempdir().unwrap();
    let config = Config {
        worktree_root: worktree_root.path().to_path_buf(),
        default_program: "claude".into(),
        profiles: vec![crate::config::Profile {
            name: "claude".into(),
            // A throwaway binary that just sleeps: the launch verification
            // (dropr:554) now requires the pane to actually stay up, so a
            // merely nonexistent path no longer works here — it never touches
            // the real `claude` CLI either way.
            program: fake_claude_binary(bin_dir.path())
                .to_string_lossy()
                .into_owned(),
            autonomous_args: Vec::new(),
            model: None,
            backend: None,
        }],
        ..Config::default()
    };
    let repo = repo_at(&repo_fixture, "myapp");

    let agent = create_agent(&repo, "hook install check", None, &config, None).unwrap();
    let _ = crate::tmux::kill_session(&agent.tmux_session);

    let settings: serde_json::Value = serde_json::from_slice(
        &std::fs::read(agent.worktree_path.join(".claude/settings.local.json")).unwrap(),
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
