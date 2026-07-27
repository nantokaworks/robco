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
        management: crate::model::ManagementMode::Auto,
        agents,
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
fn cycle_on_non_worker_selection_explains_scope() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = true;
    app.selected = 0; // OVERSEER header row — not a worker (Selection::Agent).

    cycle_selected(&mut app).unwrap();

    assert!(
        app.message
            .as_ref()
            .is_some_and(|(text, _)| text.contains("overseer management")),
        "expected a scope hint message, got {:?}",
        app.message
    );
}

#[test]
fn four_presses_return_a_user_created_worker_to_unmanaged() {
    let mut worker = worker(None, ManagementMode::Auto);

    assert_eq!(advance(&mut worker), Some(CycleStep::Auto));
    assert_eq!(worker.parent_agent_id.as_deref(), Some("overseer"));
    assert_eq!(worker.management, ManagementMode::Auto);

    assert_eq!(advance(&mut worker), Some(CycleStep::Manual));
    assert_eq!(worker.parent_agent_id.as_deref(), Some("overseer"));
    assert_eq!(worker.management, ManagementMode::Manual);

    assert_eq!(advance(&mut worker), Some(CycleStep::Unmanaged));
    assert_eq!(worker.parent_agent_id, None);

    assert_eq!(advance(&mut worker), Some(CycleStep::Auto));
    assert_eq!(worker.parent_agent_id.as_deref(), Some("overseer"));
    assert_eq!(worker.management, ManagementMode::Auto);
}

/// A worktree adopted from `worktree_root` is stored as `Manual` while nobody
/// owns it, so the mode alone would place it one step further along.
#[test]
fn an_adopted_worktree_enrolls_despite_its_stale_manual_mode() {
    let mut adopted = worker(None, ManagementMode::Manual);

    assert_eq!(cycle_step(&adopted), Some(CycleStep::Unmanaged));
    assert_eq!(advance(&mut adopted), Some(CycleStep::Auto));

    assert_eq!(adopted.parent_agent_id.as_deref(), Some("overseer"));
    assert_eq!(adopted.management, ManagementMode::Auto);
}

#[test]
fn detaching_leaves_the_mode_manual_so_a_ledger_entry_stays_frozen() {
    let mut worker = worker(Some("overseer"), ManagementMode::Manual);

    assert_eq!(advance(&mut worker), Some(CycleStep::Unmanaged));

    assert_eq!(worker.parent_agent_id, None);
    assert_eq!(worker.management, ManagementMode::Manual);
}

#[test]
fn a_worker_owned_by_another_agent_is_off_the_cycle() {
    let mut worker = worker(Some("other-parent"), ManagementMode::Manual);

    assert_eq!(cycle_step(&worker), None);
    assert_eq!(advance(&mut worker), None);

    assert_eq!(worker.parent_agent_id.as_deref(), Some("other-parent"));
    assert_eq!(worker.management, ManagementMode::Manual);
}

#[test]
fn the_legacy_overseer_parent_is_still_on_the_cycle() {
    // `chief` is the pre-rename overseer id `is_overseer_child` still accepts.
    let worker = worker(Some("chief"), ManagementMode::Auto);

    assert_eq!(cycle_step(&worker), Some(CycleStep::Auto));
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
fn a_repo_with_no_auto_worker_enrolls_its_unmanaged_worktrees_too() {
    let agents = vec![
        worker(None, ManagementMode::Manual),
        managed("b", ManagementMode::Manual),
    ];

    assert_eq!(bulk_target(&agents), Some((ManagementMode::Auto, 2)));
}

#[test]
fn standing_a_repo_down_leaves_its_unmanaged_worktrees_alone() {
    let agents = vec![
        worker(None, ManagementMode::Manual),
        managed("b", ManagementMode::Auto),
    ];

    assert_eq!(bulk_target(&agents), Some((ManagementMode::Manual, 1)));
}

#[test]
fn bulk_target_ignores_workers_owned_by_another_agent() {
    let agents = vec![
        worker(Some("other-parent"), ManagementMode::Auto),
        managed("b", ManagementMode::Manual),
    ];

    assert_eq!(bulk_target(&agents), Some((ManagementMode::Auto, 1)));
    assert_eq!(bulk_target(&agents[..1]), None);
}

#[test]
fn bulk_message_reports_a_count_not_a_bare_mode() {
    assert_eq!(
        bulk_message(4, ManagementMode::Manual),
        "4 workers set to manual"
    );
    assert_eq!(
        bulk_message(1, ManagementMode::Auto),
        "1 worker put under overseer management (auto)"
    );
    assert!(bulk_message(0, ManagementMode::Manual).starts_with("g:"));
}

#[test]
fn cycle_on_repo_row_confirms_before_switching_every_worker() {
    let mut app = app_on_repo_row(vec![
        managed("a", ManagementMode::Auto),
        managed("b", ManagementMode::Auto),
        managed("c", ManagementMode::Manual),
    ]);

    cycle_selected(&mut app).unwrap();

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

/// The repo row now has something to offer an unmanaged worktree, so the
/// "nothing to do" message is reserved for workers the overseer may not touch.
#[test]
fn cycle_on_repo_row_of_foreign_workers_explains_scope() {
    let mut app = app_on_repo_row(vec![worker(Some("other-parent"), ManagementMode::Auto)]);

    cycle_selected(&mut app).unwrap();

    assert!(matches!(app.mode, Mode::Normal));
    assert!(
        app.message
            .as_ref()
            .is_some_and(|(text, _)| text.contains("no worktrees under this repo")),
        "expected a scope hint message, got {:?}",
        app.message
    );
}

#[test]
fn cycle_on_repo_row_of_unmanaged_worktrees_offers_to_enroll_them() {
    let mut app = app_on_repo_row(vec![
        worker(None, ManagementMode::Manual),
        managed("b", ManagementMode::Manual),
    ]);

    cycle_selected(&mut app).unwrap();

    match &app.mode {
        Mode::ConfirmOverseerBulkToggle { target, count, .. } => {
            assert_eq!(*target, ManagementMode::Auto);
            assert_eq!(*count, 2);
        }
        _ => panic!("expected a bulk-toggle confirmation, got {:?}", app.message),
    }
}
