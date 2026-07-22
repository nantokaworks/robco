use std::{path::Path, process::Command};

use super::*;
use crate::model::AgentNode;

struct Fixture {
    _temp: tempfile::TempDir,
    repo: RepoNode,
    config: Config,
    agent_path: std::path::PathBuf,
}

#[test]
fn adoption_grace_period_has_a_strict_boundary() {
    assert!(should_skip_adoption(std::time::Duration::from_secs(14)));
    assert!(!should_skip_adoption(std::time::Duration::from_secs(15)));
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let repo_path = temp.path().join("repo");
        run_git(temp.path(), &["init", repo_path.to_str().unwrap()]);
        run_git(&repo_path, &["config", "user.email", "robco@example.com"]);
        run_git(&repo_path, &["config", "user.name", "Robco Test"]);
        std::fs::write(repo_path.join("README"), "test\n").unwrap();
        run_git(&repo_path, &["add", "README"]);
        run_git(&repo_path, &["commit", "-m", "initial"]);

        let worktree_root = temp.path().join("worktrees");
        let agent_path = worktree_root.join("dropr_task-749_gOQmxo");
        add_worktree(&repo_path, &agent_path, "dropr/task-749");
        let mut repo = repo_node(repo_path);
        repo.agents.push(agent_node(&repo, &agent_path));
        let config = Config {
            worktree_root,
            ..Config::default()
        };
        Self {
            _temp: temp,
            repo,
            config,
            agent_path,
        }
    }

    fn reconcile(&mut self) -> (bool, bool) {
        let worktrees = git::list_worktrees(&self.repo.path).unwrap();
        reconcile(&mut self.repo, &self.config, worktrees)
    }
}

#[test]
fn nested_worktree_is_child_instead_of_flat_agent() {
    let mut fixture = Fixture::new();
    let child = fixture.agent_path.join("child");
    add_worktree(&fixture.repo.path, &child, "child");

    let (agent_added, children_changed) = fixture.reconcile();

    assert!(!agent_added);
    assert!(children_changed);
    assert_eq!(fixture.repo.agents.len(), 1);
    assert_eq!(
        path_key(&fixture.repo.agents[0].children[0].path),
        path_key(&child)
    );
}

#[test]
fn removed_nested_worktree_clears_children_and_reports_change() {
    let mut fixture = Fixture::new();
    let child = fixture.agent_path.join("child");
    add_worktree(&fixture.repo.path, &child, "child");
    fixture.reconcile();
    assert_eq!(fixture.repo.agents[0].children.len(), 1);

    run_git(
        &fixture.repo.path,
        &["worktree", "remove", child.to_str().unwrap()],
    );
    let (agent_added, children_changed) = fixture.reconcile();

    assert!(!agent_added);
    assert!(children_changed);
    assert!(fixture.repo.agents[0].children.is_empty());
}

#[test]
fn reordered_worktrees_do_not_report_children_change() {
    let mut fixture = Fixture::new();
    let child = fixture.agent_path.join("child");
    add_worktree(&fixture.repo.path, &child, "child");
    let worktrees = git::list_worktrees(&fixture.repo.path).unwrap();

    let (_, children_changed) = reconcile(&mut fixture.repo, &fixture.config, worktrees.clone());
    assert!(children_changed);

    let (_, children_changed) = reconcile(
        &mut fixture.repo,
        &fixture.config,
        worktrees.into_iter().rev().collect(),
    );

    assert!(!children_changed);
}

#[test]
fn new_sibling_under_worktree_root_waits_for_identity_session() {
    let mut fixture = Fixture::new();
    let sibling = fixture.config.worktree_root.join("sibling");
    add_worktree(&fixture.repo.path, &sibling, "sibling");

    let (agent_added, _) = fixture.reconcile();

    assert!(!agent_added);
    assert_eq!(fixture.repo.agents.len(), 1);
}

#[test]
fn legacy_sibling_slot_nests_under_owner_instead_of_being_adopted() {
    let mut fixture = Fixture::new();
    let slot = fixture.config.worktree_root.join("dropr_task-749_slot750");
    add_worktree(&fixture.repo.path, &slot, "dropr/task-749-slot-750");
    let worktrees = git::list_worktrees(&fixture.repo.path).unwrap();
    let candidate = worktrees
        .iter()
        .find(|worktree| path_key(&worktree.path) == path_key(&slot))
        .unwrap();
    assert!(super::super::slots::is_slot_worktree(
        &candidate.path,
        candidate.branch.as_deref(),
        &fixture.repo.agents,
    ));

    let (agent_added, children_changed) =
        reconcile(&mut fixture.repo, &fixture.config, vec![candidate.clone()]);

    assert!(!agent_added);
    assert!(children_changed);
    assert_eq!(fixture.repo.agents.len(), 1);
    assert_eq!(fixture.repo.agents[0].children.len(), 1);
    assert_eq!(
        path_key(&fixture.repo.agents[0].children[0].path),
        path_key(&slot)
    );
}

#[test]
fn producer_slot_nests_and_removed_worktree_disappears() {
    let mut fixture = Fixture::new();
    let slot = fixture
        .config
        .worktree_root
        .join("dropr_task-749_gOQmxo_slot_snap");
    add_worktree(&fixture.repo.path, &slot, "slot/task-386-snap");
    std::fs::write(slot.join("slot-change"), "work\n").unwrap();
    run_git(&slot, &["add", "slot-change"]);
    run_git(&slot, &["commit", "-m", "slot work"]);

    let (agent_added, children_changed) = fixture.reconcile();

    assert!(!agent_added);
    assert!(children_changed);
    assert_eq!(fixture.repo.agents.len(), 1);
    assert_eq!(fixture.repo.agents[0].children.len(), 1);
    assert_eq!(
        fixture.repo.agents[0].children[0].ahead_behind,
        Some((0, 1))
    );

    run_git(
        &fixture.repo.path,
        &["worktree", "remove", slot.to_str().unwrap()],
    );
    let (_, children_changed) = fixture.reconcile();
    assert!(children_changed);
    assert!(fixture.repo.agents[0].children.is_empty());
}

#[test]
fn merged_producer_slot_reports_zero_commits_ahead() {
    let mut fixture = Fixture::new();
    let slot = fixture
        .config
        .worktree_root
        .join("dropr_task-749_gOQmxo_slot_pricing");
    add_worktree(&fixture.repo.path, &slot, "slot/task-387-pricing");
    std::fs::write(slot.join("pricing"), "done\n").unwrap();
    run_git(&slot, &["add", "pricing"]);
    run_git(&slot, &["commit", "-m", "pricing work"]);
    fixture.reconcile();
    assert_eq!(
        fixture.repo.agents[0].children[0].ahead_behind,
        Some((0, 1))
    );

    run_git(
        &fixture.agent_path,
        &["merge", "--ff-only", "slot/task-387-pricing"],
    );
    std::fs::write(fixture.agent_path.join("owner-change"), "next\n").unwrap();
    run_git(&fixture.agent_path, &["add", "owner-change"]);
    run_git(&fixture.agent_path, &["commit", "-m", "owner moves on"]);
    fixture.reconcile();

    assert_eq!(
        fixture.repo.agents[0].children[0].ahead_behind,
        Some((1, 0))
    );
}

#[test]
fn outside_worktree_is_ignored() {
    let mut fixture = Fixture::new();
    let outside = fixture._temp.path().join("outside");
    add_worktree(&fixture.repo.path, &outside, "outside");

    let (agent_added, children_changed) = fixture.reconcile();

    assert!(!agent_added);
    assert!(!children_changed);
    assert_eq!(fixture.repo.agents.len(), 1);
    assert!(fixture.repo.agents[0].children.is_empty());
}

#[test]
fn agent_path_is_not_its_own_child() {
    let mut fixture = Fixture::new();

    let (agent_added, children_changed) = fixture.reconcile();

    assert!(!agent_added);
    assert!(!children_changed);
    assert!(fixture.repo.agents[0].children.is_empty());
}

fn repo_node(path: std::path::PathBuf) -> RepoNode {
    RepoNode {
        path,
        name: "repo".into(),
        remote_url: None,
        pinned: false,
        agents: Vec::new(),
        dropr: None,
        dropr_tasks: Vec::new(),
        main_status: None,
        main_last_capture: None,
        main_last_change_at: None,
        main_shell_working: false,
        main_pane_pid: None,
        main_tracked_command: None,
        main_subagents_active: 0,
    }
}

fn agent_node(repo: &RepoNode, path: &Path) -> AgentNode {
    agent::adopt_worktree(
        repo,
        &Config::default(),
        path.to_path_buf(),
        Some("dropr/task-749".into()),
        None,
        None,
        None,
    )
}

fn add_worktree(repo: &Path, path: &Path, branch: &str) {
    run_git(
        repo,
        &["worktree", "add", "-b", branch, path.to_str().unwrap()],
    );
}

fn run_git(cwd: &Path, args: &[&str]) {
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
