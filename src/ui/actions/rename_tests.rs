use chrono::Local;

use super::*;
use crate::{
    config::Config,
    git,
    git::test_repo::TestRepo,
    model::{AgentNode, Status},
    registry::Registry,
};

struct Fixture {
    repo: TestRepo,
}

impl Fixture {
    fn new() -> Self {
        Self {
            repo: TestRepo::new(),
        }
    }

    fn app(&self) -> App {
        let launch = tempfile::tempdir().unwrap();
        let mut app = App::new(Registry::default(), Config::default(), launch.path().into());
        app.registry.repos = vec![crate::discover::repo_node(
            self.repo.path().to_path_buf(),
            true,
        )];
        app
    }
}

fn test_agent(worktree_path: std::path::PathBuf) -> AgentNode {
    let now = Local::now();
    AgentNode {
        id: "stable-agent".into(),
        parent_agent_id: None,
        title: "agent".into(),
        task_number: None,
        worktree_path,
        branch: "feature".into(),
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

#[test]
fn rename_prompt_is_refused_while_an_agent_is_attached() {
    let fixture = Fixture::new();
    fixture.repo.feature_branch("feature", "feature.txt");
    let worktree = fixture.repo.worktree("feature");
    let mut app = fixture.app();
    app.registry.repos[0].agents.push(test_agent(worktree));

    app.open_rename_prompt();

    assert!(matches!(app.mode, Mode::Normal));
}

#[test]
fn renaming_moves_the_directory_repairs_worktrees_and_updates_the_registry() {
    let fixture = Fixture::new();
    fixture.repo.feature_branch("feature", "feature.txt");
    let worktree = fixture.repo.worktree("feature");
    let mut app = fixture.app();
    let old_path = app.registry.repos[0].path.clone();

    // `rename_repo` reloads the registry from the sandboxed on-disk
    // state.json (shared across the whole test binary process) rather than
    // this process's in-memory snapshot, so the fixture row is pushed there
    // too — additively, never replacing what a concurrent test wrote.
    Registry::locked_update(|registry| {
        registry.repos.push(app.registry.repos[0].clone());
    })
    .unwrap();

    app.rename_repo(&old_path, "renamed");

    assert!(matches!(app.mode, Mode::Normal));
    // Matched by name, not the exact new path: `rename_repo_dir` canonicalizes
    // internally (e.g. resolving a `/var` -> `/private/var` symlink), so the
    // stored path may not equal a plain `old_path.parent().join("renamed")`.
    let renamed = app
        .registry
        .repos
        .iter()
        .find(|repo| repo.name == "renamed")
        .expect("renamed row present after reload");
    assert!(!old_path.exists());
    assert!(renamed.path.exists());
    assert!(git::worktree_is_clean(&worktree).unwrap());
}

#[test]
fn renaming_to_an_unchanged_name_does_nothing() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    let old_path = app.registry.repos[0].path.clone();
    let name = app.registry.repos[0].name.clone();

    app.rename_repo(&old_path, &name);

    assert_eq!(app.registry.repos[0].path, old_path);
    assert!(old_path.exists());
}

#[test]
fn renaming_a_stale_path_reports_it_without_touching_disk() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    let stale = fixture.repo.path().parent().unwrap().join("gone");

    app.rename_repo(&stale, "renamed");

    assert_eq!(
        app.registry.repos[0].path,
        fixture.repo.path().to_path_buf()
    );
    assert!(fixture.repo.path().exists());
}
