use super::*;
use crate::{
    config::Config,
    dropr::{DroprTaskCandidate, DroprTaskFetch, DroprWorkspace},
    model::RepoNode,
    registry::Registry,
};

fn task(display_id: &str, status: &str) -> DroprTaskCandidate {
    DroprTaskCandidate {
        display_id: display_id.to_string(),
        title: format!("Task {display_id}"),
        description: None,
        priority: String::new(),
        status: status.to_string(),
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
        host: None,
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

/// Installs an in-flight launch and hands back the sender that stands in for
/// its worker thread (dropr:508), keyed by dropr task id the way
/// `App::begin_launch` keys it (dropr:517).
fn in_flight(
    app: &mut App,
    display_id: &str,
    original_status: &str,
) -> std::sync::mpsc::Sender<super::super::dropr_task_worker::TaskLaunchResult> {
    let (job, sender) = super::super::dropr_task_worker::test_job(
        display_id,
        &format!("{display_id} Task {display_id}"),
        "/repo".into(),
        &format!("id-{display_id}"),
        original_status,
    );
    app.task_launch_jobs.insert(format!("id-{display_id}"), job);
    sender
}

fn message(app: &App) -> Option<&str> {
    app.message.as_ref().map(|(message, _)| message.as_str())
}

#[test]
fn a_background_claim_refusal_names_the_task_it_was_about() {
    // By the time this lands the operator has moved on, so the message has to
    // carry the task with it.
    let mut app = test_app();
    let sender = in_flight(&mut app, "#7", "open");
    sender
        .send(Err(TaskLaunchFailure::ClaimRefused(
            "already_claimed".into(),
        )))
        .unwrap();

    app.drain_task_launch_events();

    assert_eq!(message(&app), Some("could not claim #7: already_claimed"));
    assert!(app.task_launch_jobs.is_empty());
}

#[test]
fn a_background_spawn_failure_names_the_task_it_was_about() {
    // This one used to be a bare error string. Off-thread that names nothing.
    let mut app = test_app();
    let sender = in_flight(&mut app, "#7", "open");
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
    assert!(app.task_launch_jobs.is_empty());
}

#[test]
fn a_launch_worker_that_dies_without_answering_is_reported_against_its_task() {
    let mut app = test_app();
    let sender = in_flight(&mut app, "#7", "open");
    drop(sender);

    app.drain_task_launch_events();

    assert_eq!(
        message(&app),
        Some("launch worker for #7 terminated unexpectedly")
    );
    // The slot is freed, so the operator can try the launch again.
    assert!(app.task_launch_jobs.is_empty());
}

#[test]
fn a_launch_still_running_leaves_the_slot_and_says_nothing() {
    let mut app = test_app();
    let _sender = in_flight(&mut app, "#7", "open");

    app.drain_task_launch_events();

    assert_eq!(message(&app), None);
    assert!(app.task_launch_jobs.contains_key("id-#7"));
}

#[test]
fn a_failed_launch_reverts_the_row_to_its_original_status() {
    // The optimistic flip (`App::begin_launch`) already ran before this
    // failure lands, so the row reads `in_progress` going in — the same as
    // it would after a real keypress. A failure must put it back.
    let mut app = test_app();
    let mut repo = repo_node("/repo".into(), vec![task("#7", "open")]);
    repo.dropr_tasks.tasks[0].status = "in_progress".to_string();
    app.registry.repos = vec![repo];
    let sender = in_flight(&mut app, "#7", "open");
    sender
        .send(Err(TaskLaunchFailure::ClaimRefused(
            "already_claimed".into(),
        )))
        .unwrap();

    app.drain_task_launch_events();

    assert_eq!(app.registry.repos[0].dropr_tasks.tasks[0].status, "open");
    assert!(app.task_launch_jobs.is_empty());
}

#[test]
fn a_failed_launch_does_not_clobber_a_status_a_fresher_fetch_already_landed() {
    // A background dropr fetch can land between the keypress and the
    // failure and already show this row's real, current state — that must
    // win over a revert to a stale guess.
    let mut app = test_app();
    let mut repo = repo_node("/repo".into(), vec![task("#7", "open")]);
    repo.dropr_tasks.tasks[0].status = "blocked".to_string();
    app.registry.repos = vec![repo];
    let sender = in_flight(&mut app, "#7", "open");
    sender
        .send(Err(TaskLaunchFailure::DroprUnreachable))
        .unwrap();

    app.drain_task_launch_events();

    assert_eq!(app.registry.repos[0].dropr_tasks.tasks[0].status, "blocked");
}

#[test]
fn two_launches_in_flight_are_tracked_independently() {
    // Firing several tasks in a row is the whole point of dropr:517: neither
    // launch refuses the other, and both are visible to the quit guard.
    let mut app = test_app();
    let _first = in_flight(&mut app, "#1", "open");
    let _second = in_flight(&mut app, "#7", "open");

    assert_eq!(app.task_launch_jobs.len(), 2);
    assert_eq!(
        app.launching_tasks(),
        vec!["#1".to_string(), "#7".to_string()]
    );

    app.drain_task_launch_events();
    assert_eq!(app.task_launch_jobs.len(), 2);
}
