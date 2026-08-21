//! Recording a merge-recovery handback on the dropr task it belongs to. Split
//! out of `merge_recovery.rs` to keep that file under this project's source
//! file size limit.

use crate::overseer::{exec::COMMAND_TIMEOUT, ledger::LedgerEntry, logging::DecisionKind};

/// Records the handback on the dropr task, which is the source of truth for what
/// happened to the work.
///
/// A scribble that does not land must not abort the merge pass: the prompt has
/// already been delivered, and failing here would leave the worker working on a
/// failure Overseer then forgot it had charged.
pub(super) fn note_on_task(entry: &mut LedgerEntry, reason: &str) {
    let content = format!(
        "Overseer could not merge {} and handed the failure back to worker `{}` (handback {}): {reason}",
        entry.pr_url.as_deref().unwrap_or("the pull request"),
        entry.agent_id,
        entry.merge_recovery.charged
    );
    if let Err(error) =
        crate::dropr::scribble_create_timeout(&entry.task_id, &content, COMMAND_TIMEOUT)
    {
        // A note that did not land leaves the handback recorded nowhere an
        // operator looks, so it escalates on its own instead of riding inside
        // another decision's reason where the alert digest never reads it.
        let _ = super::log(
            entry,
            DecisionKind::Escalate,
            &format!("handback note not recorded in dropr: {error}"),
            "",
        );
    }
}
