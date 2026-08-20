//! The subtask lookup `dropr_task_drill::launch_dropr_task_from_reading` needs
//! for its `Close Dropr:` lines (dropr:479).
//!
//! Split out of `dropr_task_drill.rs` to keep that file under the 300-line
//! limit; `resolve_launch_subtasks` has no other caller.

use std::time::Duration;

use crate::{
    dropr::{self, DroprTaskCandidate, DroprTaskFetch},
    model::RepoNode,
};

/// Decides the `Close Dropr:` subtask list a launch should use, without ever
/// treating an incomplete answer as a childless task (dropr:479).
///
/// `child_count` — the task's own total, not gated by `SUBTREE_QUERY_LIMIT`
/// or `TASK_FETCH_LIMIT` the way `dropr_tasks::selectable_tasks` rows are —
/// is the ground truth this checks the cache and the scoped fetch against.
///
/// - `child_count == 0`: genuinely a leaf. No network call, same prompt shape
///   as before.
/// - `subtrees_known` already has this parent: the INFO pane's own fetch
///   answered for it, so filtering the cached rows is complete and free.
/// - Neither: the cached fetch's `SUBTREE_QUERY_LIMIT` skipped this parent.
///   `fetch` runs one scoped `task_list` call for this task alone. A
///   non-empty answer is used; an empty one is indistinguishable from that
///   same call failing (`dropr::fetch_subtasks`'s own contract), and
///   `child_count > 0` already rules out "genuinely a leaf" — so `Err(())`
///   tells the caller to refuse the launch rather than close the parent as if
///   it had no subtasks.
pub(super) fn resolve_launch_subtasks<F>(
    repo_node: &RepoNode,
    candidate: &DroprTaskCandidate,
    workspace_id: &str,
    timeout: Duration,
    fetch: F,
) -> Result<Vec<dropr::Subtask>, ()>
where
    F: FnOnce(&str, &str, Duration) -> Vec<dropr::Subtask>,
{
    if candidate.child_count == 0 {
        return Ok(Vec::new());
    }
    if repo_node.dropr_tasks.subtrees_known.contains(&candidate.id) {
        return Ok(repo_node
            .dropr_tasks
            .tasks
            .iter()
            .filter(|other| other.parent_task_id.as_deref() == Some(candidate.id.as_str()))
            .map(|other| dropr::Subtask {
                display_id: other.display_id.clone(),
            })
            .collect());
    }
    let fetched = fetch(workspace_id, &candidate.id, timeout);
    if fetched.is_empty() {
        return Err(());
    }
    Ok(fetched)
}

/// Marks one task's row as claimed and running, in the repository's cached
/// `DroprTaskFetch`, right after a launch that just claimed it in dropr
/// (dropr:482). The panel renders from this cache between background
/// refreshes, so without this the row would keep showing the state the last
/// fetch saw — inviting a second launch attempt the collision checks above
/// would then have to refuse.
///
/// A `task_id` the cache does not carry is not an error here: the row may
/// have scrolled past the display cap, or the fetch that populated the cache
/// predates this task. Either way there is no row to update, so this is a
/// silent no-op rather than a reason to fail the launch that already
/// succeeded.
pub(super) fn mark_task_in_progress(fetch: &mut DroprTaskFetch, task_id: &str) {
    if let Some(task) = fetch.tasks.iter_mut().find(|task| task.id == task_id) {
        task.status = "in_progress".to_string();
    }
}

#[cfg(test)]
#[path = "dropr_task_launch_tests.rs"]
mod tests;
