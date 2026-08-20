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

/// Installs an in-flight launch for `#7` and hands back the sender that
/// stands in for its worker thread (dropr:508).
fn in_flight(
    app: &mut App,
    display_id: &str,
) -> std::sync::mpsc::Sender<super::super::dropr_task_worker::TaskLaunchResult> {
    let (job, sender) = super::super::dropr_task_worker::test_job(
        display_id,
        &format!("{display_id} Task {display_id}"),
        "/repo".into(),
        &format!("id-{display_id}"),
    );
    app.task_launch_job = Some(job);
    sender
}

fn message(app: &App) -> Option<&str> {
    app.message.as_ref().map(|(message, _)| message.as_str())
}

#[test]
fn a_second_press_while_a_launch_is_in_flight_is_refused_naming_the_running_task() {
    // The whole launch runs off the UI thread now, so the operator can press
    // `n` again while one is still going. That press must not start a second
    // worker, and must say which task is already launching rather than
    // looking like a dropped key.
    let mut app = test_app();
    focused_at_list(&mut app, repo_node("/repo".into(), vec![task("#1")]), 0);
    let _sender = in_flight(&mut app, "#7");

    app.launch_dropr_task_from_list();

    assert_eq!(message(&app), Some("a launch is already in progress: #7"));
    // The running job is still the one that was there; nothing replaced it.
    assert_eq!(
        app.task_launch_job
            .as_ref()
            .map(|job| job.display_id.clone()),
        Some("#7".to_string())
    );
}

#[test]
fn the_reading_dialog_press_is_refused_the_same_way() {
    let mut app = test_app();
    reading(&mut app, repo_node("/repo".into(), vec![task("#1")]), 0);
    let _sender = in_flight(&mut app, "#7");

    app.launch_dropr_task_from_reading(0);

    assert_eq!(message(&app), Some("a launch is already in progress: #7"));
}

#[test]
fn a_background_claim_refusal_names_the_task_it_was_about() {
    // By the time this lands the operator has moved on, so the message has to
    // carry the task with it.
    let mut app = test_app();
    let sender = in_flight(&mut app, "#7");
    sender
        .send(Err(TaskLaunchFailure::ClaimRefused(
            "already_claimed".into(),
        )))
        .unwrap();

    app.drain_task_launch_events();

    assert_eq!(message(&app), Some("could not claim #7: already_claimed"));
    assert!(app.task_launch_job.is_none());
}

#[test]
fn a_background_spawn_failure_names_the_task_it_was_about() {
    // This one used to be a bare error string. Off-thread that names nothing.
    let mut app = test_app();
    let sender = in_flight(&mut app, "#7");
    sender
        .send(Err(TaskLaunchFailure::Spawn(
            "tmux is not installed".into(),
        )))
        .unwrap();

    app.drain_task_launch_events();

    assert_eq!(
        message(&app),
        Some("could not launch #7: tmux is not installed")
    );
    assert!(app.task_launch_job.is_none());
}

#[test]
fn a_launch_worker_that_dies_without_answering_is_reported_against_its_task() {
    let mut app = test_app();
    let sender = in_flight(&mut app, "#7");
    drop(sender);

    app.drain_task_launch_events();

    assert_eq!(
        message(&app),
        Some("launch worker for #7 terminated unexpectedly")
    );
    // The slot is freed, so the operator can try the launch again.
    assert!(app.task_launch_job.is_none());
}

#[test]
fn a_launch_still_running_leaves_the_slot_and_says_nothing() {
    let mut app = test_app();
    let _sender = in_flight(&mut app, "#7");

    app.drain_task_launch_events();

    assert_eq!(message(&app), None);
    assert!(app.task_launch_job.is_some());
}

#[test]
fn normal_quit_is_held_while_a_launch_is_in_flight() {
    // The launch outlives the key press now, so quitting mid-launch would
    // leave a claim taken in dropr with no worker behind it. Same guard an
    // in-flight merge gets.
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut app = test_app();
    let _sender = in_flight(&mut app, "#7");

    for code in [KeyCode::Char('q'), KeyCode::Esc] {
        assert!(
            !app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
                .unwrap()
        );
        assert!(app.message.as_ref().is_some_and(|(message, _)| {
            message.contains("launch in progress: #7") && message.contains("ctrl-c to force quit")
        }));
    }
}
