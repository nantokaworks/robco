use std::cell::RefCell;

use super::*;
use crate::{
    browser::Opened,
    config::Config,
    dropr::{DroprTaskCandidate, DroprTaskFetch, DroprWorkspace},
    model::RepoNode,
    registry::Registry,
};

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
        id: format!("nanoid-{}", display_id.trim_start_matches('#')),
        parent_task_id: None,
        child_count: 0,
    }
}

fn repo_node(remote_url: Option<&str>, tasks: Vec<DroprTaskCandidate>) -> RepoNode {
    RepoNode {
        path: "/repo".into(),
        name: "repo".into(),
        remote_url: remote_url.map(str::to_string),
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

fn focused_at_list(app: &mut App, repo: RepoNode, task: usize) {
    app.registry.repos = vec![repo];
    select_repo_row(app);
    app.dropr_task_focus = Some(DroprTaskFocus { task });
}

fn reading(app: &mut App, repo: RepoNode, task: usize) {
    app.registry.repos = vec![repo];
    select_repo_row(app);
    app.dropr_task_focus = Some(DroprTaskFocus { task });
    app.mode = Mode::TaskBody { task, scroll: 0 };
}

fn message(app: &App) -> Option<&str> {
    app.message.as_ref().map(|(message, _)| message.as_str())
}

#[test]
fn no_repo_selected_clears_focus_and_closes_the_dialog_without_a_message() {
    let mut app = test_app();
    app.mode = Mode::TaskBody { task: 0, scroll: 0 };

    app.open_dropr_task_from_reading(0);

    assert_eq!(app.message, None);
    assert_eq!(app.dropr_task_focus, None);
    assert!(matches!(app.mode, Mode::Normal));
}

#[test]
fn a_task_index_no_longer_listed_only_shows_a_message() {
    let mut app = test_app();
    reading(
        &mut app,
        repo_node(
            Some("https://github.com/nantokaworks/robco.git"),
            vec![task("#1")],
        ),
        5,
    );

    app.open_dropr_task_from_reading(5);

    assert_eq!(message(&app), Some("task is no longer listed"));
}

#[test]
fn a_task_without_a_dropr_id_refuses() {
    let mut app = test_app();
    let mut candidate = task("#1");
    candidate.id = String::new();
    reading(
        &mut app,
        repo_node(
            Some("https://github.com/nantokaworks/robco.git"),
            vec![candidate],
        ),
        0,
    );

    app.open_dropr_task_from_reading(0);

    assert_eq!(message(&app), Some("task is missing its dropr id"));
}

#[test]
fn no_git_remote_refuses_before_building_a_url() {
    let mut app = test_app();
    reading(&mut app, repo_node(None, vec![task("#1")]), 0);

    app.open_dropr_task_from_reading(0);

    assert_eq!(message(&app), Some("this repository has no git remote"));
}

#[test]
fn a_non_github_remote_refuses_naming_the_reason() {
    let mut app = test_app();
    reading(
        &mut app,
        repo_node(Some("https://gitlab.com/owner/repo.git"), vec![task("#1")]),
        0,
    );

    app.open_dropr_task_from_reading(0);

    assert_eq!(
        message(&app),
        Some("could not build a console URL for this repository")
    );
}

#[test]
fn a_listed_github_task_is_launched_at_its_console_url() {
    let mut app = test_app();
    reading(
        &mut app,
        repo_node(
            Some("https://github.com/nantokaworks/robco.git"),
            vec![task("#1")],
        ),
        0,
    );

    let seen = RefCell::new(None);
    app.open_dropr_task_with(0, |url| {
        *seen.borrow_mut() = Some(url.to_string());
        Ok(Opened::Launcher)
    });

    assert_eq!(
        seen.into_inner(),
        Some("https://dropr.sh/nantokaworks/robco/tasks/nanoid-1".to_string())
    );
    assert_eq!(message(&app), Some("opened #1 in the browser"));
}

#[test]
fn the_clipboard_route_shows_the_url_instead_of_claiming_a_browser_opened() {
    let mut app = test_app();
    reading(
        &mut app,
        repo_node(
            Some("https://github.com/nantokaworks/robco.git"),
            vec![task("#1")],
        ),
        0,
    );

    app.open_dropr_task_with(0, |_url| Ok(Opened::Clipboard));

    assert_eq!(
        message(&app),
        Some("copied the URL for #1: https://dropr.sh/nantokaworks/robco/tasks/nanoid-1")
    );
    assert!(app.force_redraw);
}

#[test]
fn a_launch_failure_reaches_the_operator() {
    let mut app = test_app();
    reading(
        &mut app,
        repo_node(
            Some("https://github.com/nantokaworks/robco.git"),
            vec![task("#1")],
        ),
        0,
    );

    app.open_dropr_task_with(0, |_url| Err("no display available".to_string()));

    let message = message(&app);
    assert!(message.is_some_and(|message| message.contains("#1")));
    assert!(message.is_some_and(|message| message.contains("no display available")));
}

#[test]
fn list_focus_no_repo_selected_clears_focus_without_a_message() {
    let mut app = test_app();
    app.dropr_task_focus = Some(DroprTaskFocus { task: 0 });

    app.open_dropr_task_from_list();

    assert_eq!(app.message, None);
    assert_eq!(app.dropr_task_focus, None);
}

#[test]
fn list_focus_a_listed_github_task_is_launched_at_its_console_url() {
    let mut app = test_app();
    focused_at_list(
        &mut app,
        repo_node(
            Some("https://github.com/nantokaworks/robco.git"),
            vec![task("#1")],
        ),
        0,
    );

    let seen = RefCell::new(None);
    app.open_dropr_task_with(0, |url| {
        *seen.borrow_mut() = Some(url.to_string());
        Ok(Opened::Launcher)
    });

    assert_eq!(
        seen.into_inner(),
        Some("https://dropr.sh/nantokaworks/robco/tasks/nanoid-1".to_string())
    );
    assert_eq!(message(&app), Some("opened #1 in the browser"));
}
