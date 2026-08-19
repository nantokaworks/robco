//! `Selection::DroprTask` row bookkeeping for `ui::list`, split out to keep
//! that file under this project's source file size limit (dropr:470).

use crate::model::{RepoNode, Selection};

use crate::ui::summary::dropr_tasks;

/// Stable identity for a `Selection::DroprTask` — keyed on the task's own
/// display id, not the index: a refresh can reorder tasks (priority, status
/// changes), so an index would slide the cursor onto whatever task now sits
/// at that position. Same reorder-safety rationale as `item_key`'s
/// `OverseerInbox` arm in `ui::list`.
pub(super) fn item_key(repos: &[RepoNode], repo: usize, task: usize) -> String {
    let repo_path = repos[repo].path.display().to_string();
    match dropr_tasks::selectable_tasks(&repos[repo].dropr_tasks).get(task) {
        Some(candidate) => format!("dropr-task:{repo_path}:{}", candidate.display_id),
        None => format!("dropr-task:{repo_path}:missing"),
    }
}

/// A cursor stop per selectable task, the same way `OverseerInbox` gets one
/// without a PROJECTS tree row of its own — the row itself draws only in the
/// repo's own INFO preview (`ui::tree::draw` skips `Selection::DroprTask`).
pub(super) fn push_rows(visible: &mut Vec<Selection>, repo_idx: usize, repo: &RepoNode) {
    let count = dropr_tasks::selectable_tasks(&repo.dropr_tasks).len();
    visible.extend((0..count).map(|task| Selection::DroprTask {
        repo: repo_idx,
        task,
    }));
}

#[cfg(test)]
#[path = "dropr_task_rows_tests.rs"]
mod tests;
