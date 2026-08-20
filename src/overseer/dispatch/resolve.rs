//! Resolving a bare task ref (`!run <task>`, a queued `RunTask` request) into
//! the one repository and [`Candidate`] it names.
//!
//! This is identity lookup for an operator-named task, not selection: dropr:476
//! retired the polling pass that built the *whole* ready feed to pick from on
//! its own, but an operator's own pick still needs to be resolved to a repo
//! and its dropr metadata somehow. This walks the same registered-repository /
//! dropr-workspace path the old `gather_candidates` used, stopping the moment
//! `task` is found instead of building the full candidate list.

use std::collections::BTreeSet;

use super::{
    Candidate,
    decision_log::{log_global_once, log_ready_failure, skip_unmaterialised},
};
use crate::overseer::{
    exec::COMMAND_TIMEOUT,
    logging::{self, DecisionKind},
};
use crate::{Result, dropr, dropr::READY_FETCH_LIMIT, registry::Registry};

/// Outcome of resolving a bare task ref against every registered repository.
pub(super) enum Resolved {
    /// The dropr workspace overlay could not be read at all.
    OverlayUnavailable,
    /// No registered repository's ready feed carries a task matching this ref.
    NotFound,
    Found(Box<Candidate>),
}

pub(super) fn resolve_task(
    task: &str,
    unmaterialised_logged: &mut BTreeSet<String>,
    global_hold_logged: &mut Option<String>,
) -> Result<Resolved> {
    let registry = Registry::load()?;
    let (workspaces, overlay_ok) = dropr::DroprOverlay::load_with_status_timeout(COMMAND_TIMEOUT);
    if !overlay_ok {
        log_global_once(
            DecisionKind::Skip,
            "dropr_overlay_unavailable",
            global_hold_logged,
            logging::append,
        )?;
        return Ok(Resolved::OverlayUnavailable);
    }
    for repo in &registry.repos {
        let Some(remote) = &repo.remote_url else {
            continue;
        };
        let Some(workspace) = workspaces.find_by_repo_url(remote) else {
            continue;
        };
        if skip_unmaterialised(
            &repo.path.to_string_lossy(),
            workspace,
            unmaterialised_logged,
            logging::append,
        )? {
            continue;
        }
        if !repo.path.exists() {
            continue;
        }
        let tasks = match dropr::fetch_ready_dispatch_tasks_timeout(
            &workspace.id,
            READY_FETCH_LIMIT,
            COMMAND_TIMEOUT,
        ) {
            Ok(tasks) => tasks,
            Err(error) => {
                log_ready_failure(
                    &repo.path.to_string_lossy(),
                    &workspace.id,
                    error,
                    logging::append,
                )?;
                continue;
            }
        };
        let found = tasks.into_iter().find(|row| {
            let id = if row.id.is_empty() {
                &row.task.display_id
            } else {
                &row.id
            };
            id == task || row.task.display_id == task
        });
        if let Some(row) = found {
            let task_id = if row.id.is_empty() {
                row.task.display_id.clone()
            } else {
                row.id
            };
            return Ok(Resolved::Found(Box::new(Candidate {
                task_id,
                display_id: row.task.display_id,
                title: row.task.title,
                repo: repo.path.to_string_lossy().into_owned(),
                author: row.author,
                priority: row.task.priority,
                workspace: workspace.id.clone(),
                priority_score: row.task.priority_score,
                status: row.task.status,
            })));
        }
    }
    Ok(Resolved::NotFound)
}
