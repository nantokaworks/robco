//! Re-reading a repository's dropr tasks right after robco merged in it
//! (dropr:510).
//!
//! A merge robco performed is the strongest signal there is that this
//! workspace's task list just changed: dropr's GitHub webhook closes the task
//! named by the pull request's `Close Dropr:` line a few seconds later, and
//! `DISPLAY_STATUSES` leaves closed tasks out, so the row goes away as soon as
//! robco asks again. Until it asks, the pane keeps offering work that has
//! already landed — which is how an operator starts a second worker on it.
//!
//! **This side only asks. It never writes.** dropr's webhook is the one writer
//! of that close, and it records the merged pull request in
//! `completed_details` while doing it. `dropr::task_status_update_timeout`
//! would be refused here anyway: it names `OVERSEER_AGENT_ID`, dropr only lets
//! the claim holder move a task, and a task started from the TUI is claimed by
//! `dropr_task_worker::DIRECT_LAUNCH_AGENT_ID`. Because nothing is written and
//! nothing is marked, a merge that fails leaves nothing to undo.
//!
//! The request is recorded rather than acted on, and the event loop starts the
//! fetch on the next tick. That keeps the decision — which merges count, and
//! which repositories are even linked — testable without ever spawning a
//! `dropr mcp-stdio` subprocess.

use std::path::Path;

use crate::model::RepoNode;

use super::super::App;

/// The dropr workspace a finished merge invalidates.
///
/// Both merge modes count: `m` merges the pull request itself, and the cleanup
/// key runs the same sequence for a pull request that merged elsewhere. robco
/// caused only the first, but it just learned of the second, and either way the
/// task list it shows is out of date.
///
/// `None` for a merge that failed, and `None` for a repository that is not
/// linked to a dropr workspace — neither has a task list to re-read.
pub(super) fn settle_target(
    repos: &[RepoNode],
    repo_path: &Path,
    result: &std::result::Result<(), String>,
) -> Option<String> {
    if result.is_err() {
        return None;
    }
    repos
        .iter()
        .find(|repo| repo.path == repo_path)
        .and_then(|repo| repo.dropr.as_ref())
        .map(|workspace| workspace.id.clone())
}

impl App {
    /// Records that a finished merge invalidated one repository's task list.
    ///
    /// Recording the same workspace twice before the drain keeps one entry, so
    /// merges finishing together in repositories that share a workspace cost
    /// one fetch rather than several.
    pub(super) fn note_merge_settled(
        &mut self,
        repo_path: &Path,
        result: &std::result::Result<(), String>,
    ) {
        let Some(workspace_id) = settle_target(&self.registry.repos, repo_path, result) else {
            return;
        };
        if !self.dropr_task_settle.contains(&workspace_id) {
            self.dropr_task_settle.push(workspace_id);
        }
    }

    /// Starts the fetch for every workspace a merge invalidated since the last
    /// tick.
    ///
    /// `schedule_dropr_tasks` collapses a request onto a fetch that is already
    /// in flight for the same workspace, so this cannot outrun the ordinary
    /// cycle it runs alongside.
    pub(in crate::ui) fn drain_dropr_task_settle(&mut self) {
        for workspace_id in std::mem::take(&mut self.dropr_task_settle) {
            self.schedule_dropr_tasks(workspace_id, false);
        }
    }
}

#[cfg(test)]
#[path = "dropr_task_settle_tests.rs"]
mod tests;
