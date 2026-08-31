use std::{path::Path, process::Command};

use super::*;
use crate::model::AgentNode;

pub(super) struct Fixture {
    pub(super) _temp: tempfile::TempDir,
    pub(super) repo: RepoNode,
    pub(super) config: Config,
    pub(super) agent_path: std::path::PathBuf,
}

impl Fixture {
    pub(super) fn new() -> Self {
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

    pub(super) fn reconcile(&mut self) -> (bool, bool) {
        let worktrees = git::list_worktrees(&self.repo.path).unwrap();
        reconcile(&mut self.repo, &self.config, worktrees)
    }
}

fn repo_node(path: std::path::PathBuf) -> RepoNode {
    RepoNode {
        host: None,
        path,
        name: "repo".into(),
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

pub(super) fn add_worktree(repo: &Path, path: &Path, branch: &str) {
    run_git(
        repo,
        &["worktree", "add", "-b", branch, path.to_str().unwrap()],
    );
}

pub(super) fn run_git(cwd: &Path, args: &[&str]) {
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
