//! Decision-log emission for the dispatch pass.
//!
//! Every skip, hold, and failure the pass records into `decisions.jsonl` is
//! shaped here, so `runtime` holds the pass structure and this module holds
//! what each outcome looks like on disk. The one policy decision that exists
//! only to gate its own log line — [`skip_unmaterialised`] — lives here with
//! the entry it writes.

use std::collections::BTreeSet;

use super::Candidate;
use crate::overseer::logging::{self, DecisionEntry, DecisionKind};
use crate::{Result, dropr};

pub(super) fn log_candidate(kind: DecisionKind, task: &Candidate, reason: &str) -> Result<()> {
    let mut entry = DecisionEntry::new(kind, reason);
    entry.task = Some(task.task_id.clone());
    entry.repo = Some(task.repo.clone());
    entry.source = Some("dispatch".into());
    logging::append(&entry)
}

pub(super) fn log_repo_skip<F>(repo: &str, reason: &str, append: F) -> Result<()>
where
    F: FnOnce(&DecisionEntry) -> Result<()>,
{
    let mut entry = DecisionEntry::new(DecisionKind::Skip, reason);
    entry.repo = Some(repo.into());
    entry.source = Some("dispatch".into());
    append(&entry)
}

pub(super) fn log_ready_failure<F>(
    repo: &str,
    workspace: &str,
    error: dropr::ReadyDispatchError,
    append: F,
) -> Result<()>
where
    F: FnOnce(&DecisionEntry) -> Result<()>,
{
    let mut entry = DecisionEntry::new(DecisionKind::Skip, error.reason());
    entry.repo = Some(repo.into());
    entry.source = Some("dispatch".into());
    entry.reason = format!("{}:{workspace}", error.reason());
    append(&entry)
}

pub(super) fn log_global(kind: DecisionKind, reason: &str) -> Result<()> {
    let mut entry = DecisionEntry::new(kind, reason);
    entry.source = Some("dispatch".into());
    logging::append(&entry)
}

/// Decides whether a repo's workspace can serve a ready feed at all, and
/// returns `true` when the fetch must be skipped.
///
/// A virtual (never-materialised) workspace has no task board behind it, so
/// `dropr task ready` answers HTTP 404 on every tick — a steady state, not a
/// transient fault. Retrying it each pass would spend the fetch budget and
/// write one `ready_fetch_failed` decision per minute, all day
/// (dropr:rC8ZxtZT913zsmYfnOFhs). Instead the skip is recorded once per daemon
/// run (`logged` is owned by the daemon loop) and later ticks stay quiet.
///
/// The workspace overlay is reloaded on every pass, so a workspace that gets
/// materialised later flips this check on the next tick without a daemon
/// restart; the repo is then dropped from `logged` so a workspace that ever
/// reverts to virtual logs one fresh decision rather than none.
pub(super) fn skip_unmaterialised<F>(
    repo: &str,
    workspace: &dropr::DroprWorkspace,
    logged: &mut BTreeSet<String>,
    append: F,
) -> Result<bool>
where
    F: FnOnce(&DecisionEntry) -> Result<()>,
{
    if workspace.is_materialised() {
        logged.remove(repo);
        return Ok(false);
    }
    if logged.insert(repo.to_string()) {
        log_repo_skip(repo, "workspace_not_materialised", append)?;
    }
    Ok(true)
}
