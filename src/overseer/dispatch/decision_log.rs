//! Decision-log emission for the dispatch pass.
//!
//! Every skip, hold, and failure the pass records into `decisions.jsonl` is
//! shaped here, so `runtime` holds the pass structure and this module holds
//! what each outcome looks like on disk. The one policy decision that exists
//! only to gate its own log line — [`skip_unmaterialised`] — lives here with
//! the entry it writes.

use std::collections::{BTreeMap, BTreeSet};

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

/// Records a per-candidate decision at most once per (task, reason) pair —
/// the same shape [`skip_unmaterialised`] established for `ready_fetch_failed`
/// (dropr:iCxjVrD3djgRBau23tncB), applied to every per-candidate dispatch
/// decision `execute_plan` logs, so a candidate held on the same reason for
/// an hour writes one line, not sixty.
///
/// `logged` is owned by the daemon loop (see `runtime::dispatch_pass`) and
/// pruned once per pass for task ids that left the candidate list, so a task
/// that later reappears — including a genuinely new dispatch of the same
/// task id after a prior attempt terminated — logs fresh rather than
/// inheriting a stale entry. `append` is injected the same way
/// [`skip_unmaterialised`]'s is, so the dedup decision is testable without
/// touching the real decision log.
pub(super) fn log_candidate_once<F>(
    kind: DecisionKind,
    task: &Candidate,
    reason: &str,
    logged: &mut BTreeMap<String, String>,
    append: F,
) -> Result<()>
where
    F: FnOnce(&DecisionEntry) -> Result<()>,
{
    if logged.get(&task.task_id).map(String::as_str) == Some(reason) {
        return Ok(());
    }
    logged.insert(task.task_id.clone(), reason.to_owned());
    let mut entry = DecisionEntry::new(kind, reason);
    entry.task = Some(task.task_id.clone());
    entry.repo = Some(task.repo.clone());
    entry.source = Some("dispatch".into());
    append(&entry)
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

/// Records a global (not candidate-scoped) decision at most once while the
/// same reason holds — the `dispatch_disabled` / `dropr_overlay_unavailable`
/// counterpart of [`log_candidate_once`]. `logged` is owned by the daemon
/// loop and cleared the moment a pass gets past every global gate (see
/// `runtime::dispatch_pass`), so a later recurrence — the same reason or a
/// different one — logs fresh.
pub(super) fn log_global_once<F>(
    kind: DecisionKind,
    reason: &str,
    logged: &mut Option<String>,
    append: F,
) -> Result<()>
where
    F: FnOnce(&DecisionEntry) -> Result<()>,
{
    if logged.as_deref() == Some(reason) {
        return Ok(());
    }
    *logged = Some(reason.to_owned());
    let mut entry = DecisionEntry::new(kind, reason);
    entry.source = Some("dispatch".into());
    append(&entry)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(task_id: &str) -> Candidate {
        Candidate {
            task_id: task_id.into(),
            display_id: "#1".into(),
            title: "task".into(),
            repo: "/repo".into(),
            author: "allowed".into(),
            priority: "medium".into(),
            workspace: "workspace-1".into(),
            priority_score: None,
            status: "open".into(),
            parent_task_id: None,
        }
    }

    /// The evidence this task exists for: `one_per_repo` alone wrote 15,056
    /// lines because the same held reason was logged on every poll. A
    /// candidate held on the same reason for an hour must write one line,
    /// not sixty.
    #[test]
    fn a_repeated_reason_writes_once() {
        let mut logged = BTreeMap::new();
        let mut writes = 0;
        for _ in 0..60 {
            log_candidate_once(
                DecisionKind::Skip,
                &candidate("task-1"),
                "one_per_repo",
                &mut logged,
                |_| {
                    writes += 1;
                    Ok(())
                },
            )
            .unwrap();
        }
        assert_eq!(writes, 1);
        assert_eq!(
            logged.get("task-1").map(String::as_str),
            Some("one_per_repo")
        );
    }

    #[test]
    fn a_changed_reason_writes_again() {
        let mut logged = BTreeMap::new();
        let mut reasons = Vec::new();

        log_candidate_once(
            DecisionKind::Skip,
            &candidate("task-1"),
            "one_per_repo",
            &mut logged,
            |e| {
                reasons.push(e.reason.clone());
                Ok(())
            },
        )
        .unwrap();
        log_candidate_once(
            DecisionKind::Skip,
            &candidate("task-1"),
            "one_per_repo",
            &mut logged,
            |e| {
                reasons.push(e.reason.clone());
                Ok(())
            },
        )
        .unwrap();
        // A reason that changes is a genuinely new state, so it must still
        // write a line.
        log_candidate_once(
            DecisionKind::Skip,
            &candidate("task-1"),
            "primary_slot_taken",
            &mut logged,
            |e| {
                reasons.push(e.reason.clone());
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(reasons, ["one_per_repo", "primary_slot_taken"]);
    }

    #[test]
    fn separate_tasks_are_tracked_independently() {
        let mut logged = BTreeMap::new();
        let mut writes = 0;
        for task_id in ["task-1", "task-2"] {
            log_candidate_once(
                DecisionKind::Hold,
                &candidate(task_id),
                "active_worker",
                &mut logged,
                |_| {
                    writes += 1;
                    Ok(())
                },
            )
            .unwrap();
        }
        assert_eq!(writes, 2);
        assert_eq!(logged.len(), 2);
    }

    #[test]
    fn a_repeated_global_reason_writes_once_and_a_changed_one_writes_again() {
        let mut logged = None;
        let mut reasons = Vec::new();
        for reason in ["dispatch_disabled", "dispatch_disabled", "daily_limit"] {
            log_global_once(DecisionKind::Skip, reason, &mut logged, |e| {
                reasons.push(e.reason.clone());
                Ok(())
            })
            .unwrap();
        }
        assert_eq!(reasons, ["dispatch_disabled", "daily_limit"]);
        assert_eq!(logged.as_deref(), Some("daily_limit"));
    }
}
