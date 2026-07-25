//! Decision-log writes owned by the judgment queue.
//!
//! Split out from the queue's state machine because these only ever need the
//! log path, never the queue itself — and because the queue file has to stay
//! readable while the state machine grows.

use super::{Request, result::Parsed};
use crate::{
    Result,
    overseer::logging::{self, DecisionEntry, DecisionKind},
};
use std::path::Path;

/// Reason recorded when a merge case enters the judgment queue.
///
/// Only one judgment runs at a time, so a pull request that cleared the
/// deterministic gate routinely waits several passes for its verdict. Every
/// other auto-merge outcome writes a decision; without this one the wait is the
/// single invisible state, and an operator reads the gap as auto-merge being
/// dead rather than queued. It is written on the transition into the queue —
/// `enqueue_once` refuses duplicates, so a pull request waiting across many
/// passes still records exactly one entry per revision.
pub(super) const MERGE_PENDING: &str = "judge_pending";

/// Records the verdict a completed judgment produced, one decision per subject.
pub(super) fn record(log_path: &Path, request: &Request, parsed: &Parsed) -> Result<()> {
    match (request, parsed) {
        (Request::Dispatch { approved, .. }, Parsed::Dispatch(advice)) => {
            for candidate in approved {
                let selected = advice.candidate_ids.contains(&candidate.task_id);
                let kind = if advice.fail_safe {
                    DecisionKind::Hold
                } else if selected {
                    DecisionKind::Dispatch
                } else {
                    DecisionKind::Skip
                };
                log(
                    log_path,
                    kind,
                    Some(&candidate.task_id),
                    Some(&candidate.repo),
                    &with_ignored(&advice.reason, &advice.ignored_fields),
                )?;
            }
        }
        (Request::Merge { case, .. }, Parsed::Merge(advice)) => {
            let kind = match advice.outcome {
                super::result::MergeJudgment::Allow => DecisionKind::Merge,
                super::result::MergeJudgment::Veto | super::result::MergeJudgment::Escalate => {
                    DecisionKind::Escalate
                }
            };
            log(
                log_path,
                kind,
                Some(&case.task_id),
                Some(&case.repo),
                &with_ignored(&advice.reason, &advice.ignored_fields),
            )?;
        }
        _ => unreachable!("request and judgment type must match"),
    }
    Ok(())
}

/// The verdict's reason, with the names of any keys the parser ignored.
///
/// The parser accepts a verdict that carries more than the schema asked for
/// rather than fail-safing over it, which means the extra keys are dropped. They
/// are named here so the drop is visible in the same decision entry as the
/// verdict they arrived with — an operator reading `decisions.jsonl` can see
/// that the model said more than was recorded, and where to find it.
fn with_ignored(reason: &str, ignored: &[String]) -> String {
    if ignored.is_empty() {
        return reason.to_owned();
    }
    format!("{reason} (ignored fields: {})", ignored.join(", "))
}

pub(super) fn log(
    log_path: &Path,
    kind: DecisionKind,
    task: Option<&str>,
    repo: Option<&str>,
    reason: &str,
) -> Result<()> {
    let mut entry = DecisionEntry::new(kind, reason);
    entry.task = task.map(str::to_owned);
    entry.repo = repo.map(str::to_owned);
    entry.source = Some("judge".into());
    logging::append_to(log_path, &entry)
}
