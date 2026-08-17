//! Building this pass's candidate list from the registry and dropr. Split
//! out of `runtime` to keep that file under this project's source file size
//! limit.

use std::collections::BTreeSet;

use super::{
    Candidate,
    decision_log::{log_global_once, log_ready_failure, log_repo_skip, skip_unmaterialised},
};
use crate::overseer::{
    exec::COMMAND_TIMEOUT,
    logging::{self, DecisionKind},
};
use crate::{Result, dropr, dropr::READY_FETCH_LIMIT, registry::Registry};

/// `None` means the gather itself failed (currently: the dropr workspace
/// overlay was unreachable) — distinct from `Some(vec![])`, a gather that
/// succeeded and simply found nothing ready. Callers that treat "no
/// candidates" as a board signal (the queue-drained check) must tell the two
/// apart, or an outage reads as "all done".
pub(super) fn gather_candidates(
    unmaterialised_logged: &mut BTreeSet<String>,
    global_hold_logged: &mut Option<String>,
) -> Result<Option<Vec<Candidate>>> {
    let registry = Registry::load()?;
    let (workspaces, overlay_ok) = dropr::DroprOverlay::load_with_status_timeout(COMMAND_TIMEOUT);
    if !overlay_ok {
        // Without the workspace overlay every repo would be skipped silently;
        // record the outage so an idle overseer is diagnosable from decisions.jsonl.
        log_global_once(
            DecisionKind::Skip,
            "dropr_overlay_unavailable",
            global_hold_logged,
            logging::append,
        )?;
        return Ok(None);
    }
    let mut candidates = Vec::new();
    for repo in &registry.repos {
        if repo.management == crate::model::ManagementMode::Manual {
            // An operator working a repo by hand opted it out; recording this
            // as its own skip reason (rather than falling through to
            // `workspace_unmatched`) is what keeps an idle Overseer
            // diagnosable from `decisions.jsonl` alone.
            log_repo_skip(
                &repo.path.to_string_lossy(),
                "overseer_unmanaged",
                logging::append,
            )?;
            continue;
        }
        let Some(remote) = &repo.remote_url else {
            continue;
        };
        let Some(workspace) = workspaces.find_by_repo_url(remote) else {
            log_repo_skip(
                &repo.path.to_string_lossy(),
                "workspace_unmatched",
                logging::append,
            )?;
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
            // Dispatching into a missing checkout fails the spawn and feeds
            // the failure circuit; skip stale registry entries instead.
            log_repo_skip(
                &repo.path.to_string_lossy(),
                "repo_path_missing",
                logging::append,
            )?;
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
        let repo_ids = tasks
            .iter()
            .map(|task| {
                if task.id.is_empty() {
                    task.task.display_id.clone()
                } else {
                    task.id.clone()
                }
            })
            .collect::<Vec<_>>();
        // Batched once per repo rather than once per candidate: `dropr task
        // ready` does not carry `parent_task_id` itself (dropr:yD5Gf6TX23VMvuSLFsmvO),
        // so this is the extra round-trip `gate::candidate_skip`'s ancestor
        // check needs — kept out of the gate itself, which stays pure.
        let parents = dropr::fetch_parents(&workspace.id, &repo_ids, COMMAND_TIMEOUT);
        for (task, task_id) in tasks.into_iter().zip(repo_ids) {
            let parent_task_id = parents.get(&task_id).cloned().flatten();
            candidates.push(Candidate {
                task_id,
                display_id: task.task.display_id,
                title: task.task.title,
                repo: repo.path.to_string_lossy().into_owned(),
                author: task.author,
                priority: task.task.priority,
                workspace: workspace.id.clone(),
                priority_score: task.task.priority_score,
                status: task.task.status,
                parent_task_id,
            });
        }
    }
    Ok(Some(candidates))
}

#[cfg(test)]
#[path = "gather_tests.rs"]
mod tests;
