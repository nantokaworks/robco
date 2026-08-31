use std::time::Duration;

use super::*;
use crate::{
    dropr::{DroprTaskCandidate, DroprTaskFetch, DroprWorkspace},
    model::RepoNode,
};

const TIMEOUT: Duration = Duration::from_secs(15);

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

fn subtask_row(display_id: &str, parent_id: &str) -> DroprTaskCandidate {
    let mut row = task(display_id);
    row.id = format!("id-{display_id}");
    row.parent_task_id = Some(parent_id.to_string());
    row
}

fn repo_node(tasks: Vec<DroprTaskCandidate>) -> RepoNode {
    RepoNode {
        host: None,
        path: "/repo".into(),
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

fn refusing_fetch(
    _workspace_id: &str,
    _parent_task_id: &str,
    _timeout: Duration,
) -> Vec<dropr::Subtask> {
    panic!("a genuinely childless task, or a cached-known subtree, must not fetch over the network")
}

#[test]
fn a_genuinely_childless_task_needs_no_fetch() {
    let mut candidate = task("#1");
    candidate.child_count = 0;
    let repo = repo_node(vec![candidate.clone()]);

    let subtasks =
        resolve_launch_subtasks(&repo, &candidate, "workspace-1", TIMEOUT, refusing_fetch).unwrap();

    assert!(subtasks.is_empty());
}

#[test]
fn a_parent_with_a_fetched_subtree_uses_the_cache() {
    let mut candidate = task("#1");
    candidate.child_count = 1;
    let child = subtask_row("#2", &candidate.id);
    let mut repo = repo_node(vec![candidate.clone(), child]);
    repo.dropr_tasks.subtrees_known.insert(candidate.id.clone());

    let subtasks =
        resolve_launch_subtasks(&repo, &candidate, "workspace-1", TIMEOUT, refusing_fetch).unwrap();

    assert_eq!(
        subtasks,
        vec![dropr::Subtask {
            display_id: "#2".to_string()
        }]
    );
}

#[test]
fn a_parent_with_an_unfetched_subtree_takes_a_scoped_fetch() {
    let mut candidate = task("#1");
    candidate.child_count = 1;
    let repo = repo_node(vec![candidate.clone()]);

    let subtasks = resolve_launch_subtasks(
        &repo,
        &candidate,
        "workspace-1",
        TIMEOUT,
        |workspace, parent, timeout| {
            assert_eq!(workspace, "workspace-1");
            assert_eq!(parent, candidate.id);
            assert_eq!(timeout, TIMEOUT);
            vec![dropr::Subtask {
                display_id: "#2".to_string(),
            }]
        },
    )
    .unwrap();

    assert_eq!(
        subtasks,
        vec![dropr::Subtask {
            display_id: "#2".to_string()
        }]
    );
}

#[test]
fn an_unfetched_subtree_that_comes_back_empty_refuses_rather_than_launching_childless() {
    let mut candidate = task("#1");
    candidate.child_count = 1;
    let repo = repo_node(vec![candidate.clone()]);

    let result = resolve_launch_subtasks(&repo, &candidate, "workspace-1", TIMEOUT, |_, _, _| {
        Vec::new()
    });

    assert_eq!(result, Err(()));
}

#[test]
fn marking_a_launched_task_updates_only_that_row() {
    let launched = task("#1");
    let other = task("#2");
    let mut fetch = repo_node(vec![launched.clone(), other.clone()]).dropr_tasks;

    mark_task_in_progress(&mut fetch, &launched.id);

    assert_eq!(fetch.tasks[0].status, "in_progress");
    assert_eq!(fetch.tasks[1].status, "open");
}

#[test]
fn marking_a_task_id_the_cache_does_not_carry_is_a_no_op() {
    let cached = task("#1");
    let mut fetch = repo_node(vec![cached.clone()]).dropr_tasks;

    mark_task_in_progress(&mut fetch, "id-not-in-cache");

    assert_eq!(fetch.tasks[0].status, cached.status);
}

#[test]
fn reverting_puts_back_exactly_what_marking_flipped() {
    let launched = task("#1");
    let mut fetch = repo_node(vec![launched.clone()]).dropr_tasks;
    mark_task_in_progress(&mut fetch, &launched.id);

    revert_task_in_progress(&mut fetch, &launched.id, "blocked");

    assert_eq!(fetch.tasks[0].status, "blocked");
}

#[test]
fn reverting_a_row_a_fresher_fetch_already_moved_on_is_a_no_op() {
    // A real background fetch can land between the optimistic mark and a
    // launch failure and already show this row's current, real status — that
    // must win over a revert to a stale guess about what it used to say.
    let mut candidate = task("#1");
    candidate.status = "blocked".to_string();
    let mut fetch = repo_node(vec![candidate.clone()]).dropr_tasks;

    revert_task_in_progress(&mut fetch, &candidate.id, "open");

    assert_eq!(fetch.tasks[0].status, "blocked");
}
