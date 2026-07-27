use std::{path::Path, process::Command};

use chrono::Local;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::{
    config::Config,
    model::{AgentNode, RepoNode, Status},
    registry::Registry,
};

struct Fixture {
    _temp: tempfile::TempDir,
    repo: RepoNode,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let repo_path = temp.path().join("repo");
        run_git(temp.path(), &["init", repo_path.to_str().unwrap()]);
        run_git(&repo_path, &["config", "user.email", "robco@example.com"]);
        run_git(&repo_path, &["config", "user.name", "Robco Test"]);
        std::fs::write(repo_path.join("README"), "initial\n").unwrap();
        run_git(&repo_path, &["add", "README"]);
        run_git(&repo_path, &["commit", "-m", "initial"]);

        let worktree = temp.path().join("agent");
        run_git(
            &repo_path,
            &[
                "worktree",
                "add",
                "-b",
                "feature/agent",
                worktree.to_str().unwrap(),
            ],
        );
        let agent = test_agent(worktree);
        Self {
            _temp: temp,
            repo: test_repo(repo_path, agent),
        }
    }

    fn app(&self) -> App {
        let launch = tempfile::tempdir().unwrap();
        let mut app = App::new(Registry::default(), Config::default(), launch.path().into());
        app.registry.repos = vec![self.repo.clone()];
        app
    }
}

fn test_agent(worktree_path: std::path::PathBuf) -> AgentNode {
    let now = Local::now();
    AgentNode {
        management: crate::model::ManagementMode::Manual,
        id: "stable-agent".into(),
        parent_agent_id: None,
        title: "agent".into(),
        worktree_path,
        branch: "feature/agent".into(),
        base_commit: String::new(),
        program: "codex".into(),
        claude_session_id: None,
        profile: None,
        tmux_session: "robco_test_missing".into(),
        created_at: now,
        updated_at: now,
        status: Status::Running,
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

fn test_repo(path: std::path::PathBuf, agent: AgentNode) -> RepoNode {
    RepoNode {
        path,
        name: "repo".into(),
        remote_url: None,
        pinned: false,
        management: crate::model::ManagementMode::Auto,
        agents: vec![agent],
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
    }
}

fn run_git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(["-C"])
        .arg(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn dirty_kill_opens_dialog_with_files_and_keeps_row() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.repo.agents[0].worktree_path.join("README"),
        "dirty\n",
    )
    .unwrap();
    let mut app = fixture.app();

    app.kill_agent(0, 0).unwrap();

    assert_eq!(app.registry.repos[0].agents.len(), 1);
    assert!(matches!(
        &app.mode,
        Mode::ErrorDialog { title, lines, force_kill: Some(_) }
            if title == "kill failed"
                && lines.iter().any(|line| line.contains("tracked changes"))
                && lines.iter().any(|line| line.contains("README"))
    ));
}

#[test]
fn dialog_dismisses_without_changing_registry_or_worktree() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.repo.agents[0].worktree_path.join("README"),
        "dirty\n",
    )
    .unwrap();
    let worktree = fixture.repo.agents[0].worktree_path.clone();
    let mut app = fixture.app();
    app.kill_agent(0, 0).unwrap();

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();

    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(app.registry.repos[0].agents.len(), 1);
    assert!(worktree.exists());
}

#[test]
fn untracked_only_failure_is_force_recoverable() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.repo.agents[0].worktree_path.join("new.txt"),
        "new\n",
    )
    .unwrap();
    let mut app = fixture.app();

    app.kill_agent(0, 0).unwrap();

    assert!(matches!(
        &app.mode,
        Mode::ErrorDialog { lines, force_kill: Some(_), .. }
            if lines.iter().any(|line| line.contains("new.txt"))
    ));
}

#[test]
fn force_kill_removes_an_untracked_worktree() {
    let fixture = Fixture::new();
    let worktree = fixture.repo.agents[0].worktree_path.clone();
    std::fs::write(worktree.join("new.txt"), "new\n").unwrap();

    assert!(matches!(
        agent::kill_agent(&fixture.repo, &fixture.repo.agents[0], false),
        Err(Error::Command {
            context: "git worktree remove",
            ..
        })
    ));
    agent::kill_agent(&fixture.repo, &fixture.repo.agents[0], true).unwrap();

    assert!(!worktree.exists());
    assert!(git::branch_exists(&fixture.repo.path, "feature/agent").unwrap());
}

#[test]
fn force_kill_removes_a_dirty_tracked_worktree() {
    let fixture = Fixture::new();
    let worktree = fixture.repo.agents[0].worktree_path.clone();
    std::fs::write(worktree.join("README"), "dirty\n").unwrap();

    assert!(matches!(
        agent::kill_agent(&fixture.repo, &fixture.repo.agents[0], false),
        Err(Error::DirtyWorktree(_))
    ));
    agent::kill_agent(&fixture.repo, &fixture.repo.agents[0], true).unwrap();

    assert!(!worktree.exists());
}

#[test]
fn stale_force_target_is_a_safe_noop() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    app.registry.repos[0].agents.clear();
    app.mode = Mode::ErrorDialog {
        title: "kill failed".into(),
        lines: vec!["failed".into()],
        force_kill: Some(ForceKillTarget {
            repo_path: fixture.repo.path.clone(),
            agent_id: "stable-agent".into(),
        }),
    };

    app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
        .unwrap();

    assert!(matches!(app.mode, Mode::Normal));
}

#[test]
fn force_recoverable_matches_only_supported_kill_failures() {
    assert!(force_recoverable(&Error::DirtyWorktree("/tmp/wt".into())));
    assert!(force_recoverable(&Error::Command {
        context: "git worktree remove",
        stderr: "dirty".into(),
    }));
    assert!(!force_recoverable(&Error::ChildWorktreesPresent(
        "/tmp/wt".into()
    )));
    assert!(!force_recoverable(&Error::Command {
        context: "git branch -D",
        stderr: "failed".into(),
    }));
}

#[test]
fn branch_delete_failure_uses_error_dialog_without_force() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    app.registry.repos[0].agents[0].branch = "missing-branch".into();

    app.delete_agent_branch(0, 0).unwrap();

    assert!(matches!(
        app.mode,
        Mode::ErrorDialog {
            force_kill: None,
            ..
        }
    ));
    assert_eq!(app.registry.repos[0].agents.len(), 1);
}
