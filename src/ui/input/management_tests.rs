use super::*;
use crate::{
    config::Config,
    model::{RepoNode, Status},
    registry::Registry,
};
use chrono::Local;

fn worker(parent: Option<&str>, management: ManagementMode) -> AgentNode {
    AgentNode {
        id: "worker-1".into(),
        parent_agent_id: parent.map(str::to_string),
        management,
        title: "worker".into(),
        worktree_path: "/tmp/worker-1".into(),
        branch: "worker-1".into(),
        base_commit: "abc123".into(),
        program: "claude".into(),
        claude_session_id: None,
        profile: None,
        tmux_session: "worker-1".into(),
        created_at: Local::now(),
        updated_at: Local::now(),
        status: Status::Idle,
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
        subagents: vec![],
        children: vec![],
    }
}

fn managed(id: &str, management: ManagementMode) -> AgentNode {
    AgentNode {
        id: id.into(),
        ..worker(Some(crate::overseer::OVERSEER_AGENT_ID), management)
    }
}

fn repo(agents: Vec<AgentNode>) -> RepoNode {
    RepoNode {
        name: "robco".into(),
        path: "/tmp/robco".into(),
        remote_url: None,
        pinned: false,
        agents,
        dropr: None,
        dropr_tasks: Vec::new(),
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

/// App with one repo selected on its repo row, ready for a `g` press. The
/// repo is pushed after construction so `App::new`'s unmanaged-worktree
/// prune (which would also persist the registry) leaves it alone.
fn app_on_repo_row(agents: Vec<AgentNode>) -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.registry.repos.push(repo(agents));
    app.selected = app
        .visible()
        .iter()
        .position(|item| matches!(item, Selection::Repo(0)))
        .expect("repo row is visible");
    app
}

#[test]
fn toggle_on_non_worker_selection_explains_scope() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = true;
    app.selected = 0; // OVERSEER header row — not a worker (Selection::Agent).

    toggle_selected(&mut app).unwrap();

    assert!(
        app.message
            .as_ref()
            .is_some_and(|(text, _)| text.contains("overseer worker")),
        "expected a scope hint message, got {:?}",
        app.message
    );
}

#[test]
fn only_overseer_workers_toggle() {
    let mut mode = ManagementMode::Auto;
    assert!(toggle_mode(Some("overseer"), &mut mode));
    assert_eq!(mode, ManagementMode::Manual);
    assert!(toggle_mode(Some("chief"), &mut mode));
    assert_eq!(mode, ManagementMode::Auto);
    assert!(!toggle_mode(None, &mut mode));
    assert_eq!(mode, ManagementMode::Auto);
}

#[test]
fn one_auto_worker_stands_the_whole_repo_down() {
    let agents = vec![
        managed("a", ManagementMode::Manual),
        managed("b", ManagementMode::Auto),
        managed("c", ManagementMode::Manual),
    ];

    assert_eq!(bulk_target(&agents), Some((ManagementMode::Manual, 1)));
}

#[test]
fn a_fully_manual_repo_goes_back_to_auto() {
    let agents = vec![
        managed("a", ManagementMode::Manual),
        managed("b", ManagementMode::Manual),
    ];

    assert_eq!(bulk_target(&agents), Some((ManagementMode::Auto, 2)));
}

#[test]
fn bulk_target_ignores_workers_the_overseer_does_not_own() {
    let agents = vec![
        worker(None, ManagementMode::Auto),
        worker(Some("other-parent"), ManagementMode::Auto),
        managed("c", ManagementMode::Manual),
    ];

    assert_eq!(bulk_target(&agents), Some((ManagementMode::Auto, 1)));
    assert_eq!(bulk_target(&agents[..2]), None);
}

#[test]
fn bulk_message_reports_a_count_not_a_bare_mode() {
    assert_eq!(
        bulk_message(4, ManagementMode::Manual),
        "4 workers set to manual"
    );
    assert_eq!(
        bulk_message(1, ManagementMode::Auto),
        "1 worker set to auto"
    );
    assert!(bulk_message(0, ManagementMode::Manual).starts_with("g:"));
}

#[test]
fn toggle_on_repo_row_confirms_before_switching_every_worker() {
    let mut app = app_on_repo_row(vec![
        managed("a", ManagementMode::Auto),
        managed("b", ManagementMode::Auto),
        managed("c", ManagementMode::Manual),
    ]);

    toggle_selected(&mut app).unwrap();

    match &app.mode {
        Mode::ConfirmOverseerBulkToggle {
            repo_name,
            target,
            count,
            ..
        } => {
            assert_eq!(repo_name, "robco");
            assert_eq!(*target, ManagementMode::Manual);
            assert_eq!(*count, 2);
        }
        _ => panic!("expected a bulk-toggle confirmation, got {:?}", app.message),
    }
}

#[test]
fn toggle_on_repo_row_without_managed_workers_explains_scope() {
    let mut app = app_on_repo_row(vec![worker(None, ManagementMode::Auto)]);

    toggle_selected(&mut app).unwrap();

    assert!(matches!(app.mode, Mode::Normal));
    assert!(
        app.message
            .as_ref()
            .is_some_and(|(text, _)| text.contains("no overseer-managed workers")),
        "expected a scope hint message, got {:?}",
        app.message
    );
}

#[test]
fn enroll_sets_overseer_parent_and_auto_management() {
    let mut worker = worker(None, ManagementMode::Manual);

    assert_eq!(enroll(&mut worker), EnrollOutcome::Enrolled);

    assert_eq!(worker.parent_agent_id.as_deref(), Some("overseer"));
    assert_eq!(worker.management, ManagementMode::Auto);
}

#[test]
fn enroll_preserves_management_when_already_managed() {
    let mut worker = worker(Some("overseer"), ManagementMode::Manual);

    assert_eq!(enroll(&mut worker), EnrollOutcome::AlreadyManaged);

    assert_eq!(worker.parent_agent_id.as_deref(), Some("overseer"));
    assert_eq!(worker.management, ManagementMode::Manual);
}

#[test]
fn exclude_clears_overseer_parent() {
    let mut worker = worker(Some("overseer"), ManagementMode::Manual);

    assert_eq!(exclude(&mut worker), ExcludeOutcome::Excluded);

    assert_eq!(worker.parent_agent_id, None);
    assert_eq!(worker.management, ManagementMode::Manual);
}

#[test]
fn exclude_preserves_non_overseer_parent() {
    let mut worker = worker(Some("other-parent"), ManagementMode::Manual);

    assert_eq!(exclude(&mut worker), ExcludeOutcome::NotOverseerChild);

    assert_eq!(worker.parent_agent_id.as_deref(), Some("other-parent"));
}
