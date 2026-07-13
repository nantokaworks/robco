use std::{path::Path, process::Command};

use super::*;
use crate::model::AgentNode;

struct Fixture {
    _temp: tempfile::TempDir,
    repo: RepoNode,
    config: Config,
    agent_path: std::path::PathBuf,
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
        let agent_path = worktree_root.join("agent");
        add_worktree(&repo_path, &agent_path, "agent");
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
fn sibling_under_worktree_root_is_flat_agent() {
    let mut fixture = Fixture::new();
    let sibling = fixture.config.worktree_root.join("sibling");
    add_worktree(&fixture.repo.path, &sibling, "sibling");

    let (agent_added, _) = fixture.reconcile();

    assert!(agent_added);
    assert_eq!(fixture.repo.agents.len(), 2);
    assert!(
        fixture
            .repo
            .agents
            .iter()
            .any(|agent| path_key(&agent.worktree_path) == path_key(&sibling))
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
        Some("agent".into()),
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
