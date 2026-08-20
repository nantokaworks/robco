use super::*;
use crate::{
    config::Config,
    dropr::{DroprTaskCandidate, DroprTaskFetch, DroprWorkspace},
    git::test_repo::TestRepo,
    model::{AgentNode, RepoNode},
    registry::Registry,
};

fn agent_node(id: &str, title: &str, task_number: Option<&str>) -> AgentNode {
    let now = chrono::Local::now();
    AgentNode {
        id: id.to_string(),
        parent_agent_id: None,
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

fn reading(app: &mut App, repo: RepoNode, task: usize) {
    app.registry.repos = vec![repo];
    select_repo_row(app);
    app.dropr_task_focus = Some(DroprTaskFocus { task });
    app.mode = Mode::TaskBody { task, scroll: 0 };
}

fn focused_at_list(app: &mut App, repo: RepoNode, task: usize) {
    app.registry.repos = vec![repo];
    select_repo_row(app);
    app.dropr_task_focus = Some(DroprTaskFocus { task });
}

#[test]
fn no_repo_selected_clears_focus_and_closes_the_dialog_without_a_message() {
    // A stale focus outliving the repository row it was entered from (the
    // cursor moved some other way while it was set) drops silently rather
    // than acting on a selection that no longer names a task list.
    let mut app = test_app();
    app.mode = Mode::TaskBody { task: 0, scroll: 0 };

    app.launch_dropr_task_from_reading(0);

    assert_eq!(app.message, None);
    assert_eq!(app.dropr_task_focus, None);
    assert!(matches!(app.mode, Mode::Normal));
}

#[test]
fn a_task_index_no_longer_listed_only_shows_a_message() {
    let mut app = test_app();
    reading(&mut app, repo_node("/repo".into(), vec![task("#1")]), 5);

    app.launch_dropr_task_from_reading(5);

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
    reading(&mut app, repo, 0);

    app.launch_dropr_task_from_reading(0);

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
    reading(&mut app, repo_node("/repo".into(), vec![candidate]), 0);

    app.launch_dropr_task_from_reading(0);

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
    reading(&mut app, repo, 0);

    app.launch_dropr_task_from_reading(0);

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
    reading(
        &mut app,
        repo_node(repo.path().to_path_buf(), vec![candidate]),
        0,
    );

    app.launch_dropr_task_from_reading(0);

    let message = app.message.as_ref().map(|(message, _)| message.as_str());
    assert!(message.is_some_and(|message| message.contains(&branch)));
}

#[test]
fn list_focus_no_repo_selected_clears_focus_without_a_message() {
    // Mirrors `no_repo_selected_clears_focus_and_closes_the_dialog_without_a_message`
    // above, from the list entry point (dropr:482).
    let mut app = test_app();
    app.dropr_task_focus = Some(DroprTaskFocus { task: 0 });

    app.launch_dropr_task_from_list();

    assert_eq!(app.message, None);
    assert_eq!(app.dropr_task_focus, None);
}

#[test]
fn list_focus_a_task_index_no_longer_listed_only_shows_a_message() {
    let mut app = test_app();
    focused_at_list(&mut app, repo_node("/repo".into(), vec![task("#1")]), 5);

    app.launch_dropr_task_from_list();

    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("task is no longer listed")
    );
}

#[test]
fn list_focus_a_live_worker_for_the_task_refuses_naming_it() {
    // Same refusal `a_live_worker_for_the_task_refuses_naming_it` proves from
    // the reading dialog — `n` at the list level shares the same launch path
    // (dropr:482), so it must refuse the same way.
    let mut app = test_app();
    let mut repo = repo_node("/repo".into(), vec![task("#1")]);
    repo.agents
        .push(agent_node("existing-worker", "existing worker", Some("1")));
    focused_at_list(&mut app, repo, 0);

    app.launch_dropr_task_from_list();

    let message = app.message.as_ref().map(|(message, _)| message.as_str());
    assert!(message.is_some_and(|message| message.contains("existing worker")));
}
