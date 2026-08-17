//! Shared plumbing `repo_watch_advisory` and `repo_watch_dependabot` both
//! need: the marker convention that makes a re-run idempotent, and the write
//! that files the task and reports it through the existing Discord path.

use std::time::Duration;

use crate::{
    dropr,
    overseer::logging::{self, DecisionEntry, DecisionKind},
};

const WRITE_TIMEOUT: Duration = Duration::from_secs(20);

/// Whether an open task on `workspace_id`'s board already carries this exact
/// condition's `marker`.
///
/// `dropr::fetch_repo_tasks` only returns `open` / `in_progress` / `blocked`
/// rows, so a match here is by definition not yet resolved. The dedup key
/// lives in the marker embedded in the task title, not in local state, so a
/// restart or a lost state file can never cause a duplicate — the board
/// itself is the only fact this checks (dropr task #430's "no duplicate
/// tasks across restarts" acceptance criterion).
///
/// `None` when the board could not be read at all: the caller must not file
/// in that case, since it cannot rule out a duplicate already sitting there.
///
/// Bounded the same way `fetch_repo_tasks` bounds every caller: at most
/// `TASK_FETCH_LIMIT` root tasks and `SUBTREE_QUERY_LIMIT` parents' worth of
/// subtasks. A workspace with more open root tasks than that could in theory
/// miss an older marker outside the window; in practice a watched
/// repository's open-task count stays well under it.
pub(super) fn already_open(workspace_id: &str, marker: &str) -> Option<bool> {
    let fetch = dropr::fetch_repo_tasks(workspace_id);
    if !fetch.answered {
        return None;
    }
    Some(fetch.tasks.iter().any(|task| task.title.contains(marker)))
}

/// Files the task and logs a `daemon_event` decision so it reaches Discord
/// through the same path `daemon::discord_events` dispatch/merge events
/// already use — see `discord::notifications::from_decision`.
pub(super) fn file(
    workspace_id: &str,
    repo_name: &str,
    title: &str,
    description: &str,
    event_reason: &'static str,
) -> Result<(), String> {
    dropr::task_create_timeout(workspace_id, title, Some(description), WRITE_TIMEOUT)
        .map_err(|error| format!("dropr task_create failed: {error}"))?;
    let mut entry = DecisionEntry::new(DecisionKind::Dispatch, event_reason);
    entry.source = Some("daemon_event".into());
    entry.repo = Some(repo_name.to_string());
    logging::append(&entry).map_err(|error| format!("decision log write failed: {error}"))
}
