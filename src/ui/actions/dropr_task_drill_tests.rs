use super::*;
use crate::{
    config::Config,
    dropr::{DroprTaskCandidate, DroprTaskFetch, DroprWorkspace},
    git::test_repo::TestRepo,
    model::{AgentNode, ManagementMode, RepoNode},
    registry::Registry,
};

fn agent_node(id: &str, title: &str, task_number: Option<&str>) -> AgentNode {
    let now = chrono::Local::now();
    AgentNode {
        id: id.to_string(),
        parent_agent_id: None,
        management: ManagementMode::Manual,
        title: title.to_string(),
        task_number: task_number.map(str::to_string),
        worktree_path: format!("/tmp/{id}").into(),
        branch: id.to_string(),
        base_commit: String::new(),
        program: "claude".to_string(),
        claude_session_id: None,
        profile: None,
        tmux_session: id.to_string(),
        created_at: now,
        updated_at: now,
        status: Default::default(),
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

fn task(display_id: &str) -> DroprTaskCandidate {
    DroprTaskCandidate {
        display_id: display_id.to_string(),
        title: format!("Task {display_id}"),
        description: None,
        priority: String::new(),
        status: "open".to_string(),
        priority_score: None,
        blocked_reason: None,
        updated_at: None,
        id: format!("id-{display_id}"),
        parent_task_id: None,
        child_count: 0,
    }
}

fn repo_node(path: std::path::PathBuf, tasks: Vec<DroprTaskCandidate>) -> RepoNode {
    RepoNode {
        path,
        name: "repo".into(),
        remote_url: None,
        pinned: true,
        management: ManagementMode::Auto,
        agents: Vec::new(),
        dropr: Some(DroprWorkspace {
            kind: "materialised".into(),
            id: "workspace-1".into(),
            name: "workspace".into(),
            repo_url: String::new(),
        }),
        dropr_tasks: DroprTaskFetch {
            tasks,
            problems: Vec::new(),
            answered: true,
            subtrees_known: Default::default(),
        },
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

fn test_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    App::new(Registry::default(), Config::default(), temp.path().into())
}

/// Points `app.selected` at `Selection::Repo(0)`, wherever `App::visible`
/// actually placed it — the fixture's repo path is not under the app's
/// launch dir, so it renders under "other locations" rather than at index 0.
fn select_repo_row(app: &mut App) {
    app.selected = app
        .visible()
        .iter()
        .position(|selection| matches!(selection, Selection::Repo(0)))
        .expect("repo row is visible");
}

fn focused_at_body(app: &mut App, repo: RepoNode, task: usize) {
    app.registry.repos = vec![repo];
    select_repo_row(app);
    app.dropr_task_focus = Some(DroprTaskFocus::Body { task });
}

#[test]
fn no_repo_selected_clears_focus_without_a_message() {
    // A stale focus outliving the repository row it was entered from (the
    // cursor moved some other way while it was set) drops silently rather
    // than acting on a selection that no longer names a task list.
    let mut app = test_app();
    app.dropr_task_focus = Some(DroprTaskFocus::Body { task: 0 });

    app.launch_dropr_task_from_body();

    assert_eq!(app.message, None);
    assert_eq!(app.dropr_task_focus, None);
}

#[test]
fn a_task_index_no_longer_listed_only_shows_a_message() {
    let mut app = test_app();
    focused_at_body(&mut app, repo_node("/repo".into(), vec![task("#1")]), 5);

    app.launch_dropr_task_from_body();

    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("task is no longer listed")
    );
}

#[test]
fn no_linked_workspace_refuses_before_touching_dropr() {
    let mut app = test_app();
    let mut repo = repo_node("/repo".into(), vec![task("#1")]);
    repo.dropr = None;
    focused_at_body(&mut app, repo, 0);

    app.launch_dropr_task_from_body();

    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("no dropr workspace linked to this repo")
    );
}

#[test]
fn a_task_without_a_dropr_id_refuses() {
    let mut app = test_app();
    let mut candidate = task("#1");
    candidate.id = String::new();
    focused_at_body(&mut app, repo_node("/repo".into(), vec![candidate]), 0);

    app.launch_dropr_task_from_body();

    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("task is missing its dropr id")
    );
}

#[test]
fn a_live_worker_for_the_task_refuses_naming_it() {
    let mut app = test_app();
    let mut repo = repo_node("/repo".into(), vec![task("#1")]);
    repo.agents
        .push(agent_node("existing-worker", "existing worker", Some("1")));
    focused_at_body(&mut app, repo, 0);

    app.launch_dropr_task_from_body();

    let message = app.message.as_ref().map(|(message, _)| message.as_str());
    assert!(message.is_some_and(|message| message.contains("existing worker")));
}

#[test]
fn an_existing_branch_for_the_task_refuses_naming_it() {
    let repo = TestRepo::new();
    let candidate = task("#1");
    let title = format!("{} {}", candidate.display_id, candidate.title);
    let branch = agent::worker_branch_name(&Config::default(), "repo", &title, None);
    repo.feature_branch(&branch, "claimed.txt");

    let mut app = test_app();
    focused_at_body(
        &mut app,
        repo_node(repo.path().to_path_buf(), vec![candidate]),
        0,
    );

    app.launch_dropr_task_from_body();

    let message = app.message.as_ref().map(|(message, _)| message.as_str());
    assert!(message.is_some_and(|message| message.contains(&branch)));
}

#[test]
fn entering_the_task_list_starts_on_the_first_row() {
    let mut app = test_app();
    app.enter_dropr_task_list();
    assert_eq!(app.dropr_task_focus, Some(DroprTaskFocus::List { task: 0 }));
}

#[test]
fn leaving_the_task_list_clears_focus() {
    let mut app = test_app();
    app.dropr_task_focus = Some(DroprTaskFocus::List { task: 2 });
    app.leave_dropr_task_list();
    assert_eq!(app.dropr_task_focus, None);
}

#[test]
fn moving_the_task_cursor_clamps_to_what_is_listed() {
    let mut app = test_app();
    app.registry.repos = vec![repo_node("/repo".into(), vec![task("#1"), task("#2")])];
    select_repo_row(&mut app);
    app.dropr_task_focus = Some(DroprTaskFocus::List { task: 0 });

    app.move_dropr_task_cursor(-1);
    assert_eq!(app.dropr_task_focus, Some(DroprTaskFocus::List { task: 0 }));

    app.move_dropr_task_cursor(1);
    assert_eq!(app.dropr_task_focus, Some(DroprTaskFocus::List { task: 1 }));

    app.move_dropr_task_cursor(1);
    assert_eq!(app.dropr_task_focus, Some(DroprTaskFocus::List { task: 1 }));
}

#[test]
fn opening_a_stale_task_row_does_nothing() {
    let mut app = test_app();
    app.registry.repos = vec![repo_node("/repo".into(), vec![task("#1")])];
    select_repo_row(&mut app);
    app.dropr_task_focus = Some(DroprTaskFocus::List { task: 5 });

    app.open_dropr_task_body();

    assert_eq!(app.dropr_task_focus, Some(DroprTaskFocus::List { task: 5 }));
}

#[test]
fn opening_a_listed_task_row_moves_to_its_body() {
    let mut app = test_app();
    app.registry.repos = vec![repo_node("/repo".into(), vec![task("#1")])];
    select_repo_row(&mut app);
    app.dropr_task_focus = Some(DroprTaskFocus::List { task: 0 });

    app.open_dropr_task_body();

    assert_eq!(app.dropr_task_focus, Some(DroprTaskFocus::Body { task: 0 }));
}

#[test]
fn closing_the_body_returns_to_the_list_on_the_same_task() {
    let mut app = test_app();
    app.dropr_task_focus = Some(DroprTaskFocus::Body { task: 3 });
    app.close_dropr_task_body();
    assert_eq!(app.dropr_task_focus, Some(DroprTaskFocus::List { task: 3 }));
}
