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
///
/// An entry with no known dropr task (`LedgerEntry::dropr_task_id` is `None`)
/// has nowhere to record the handback, so this does not call dropr at all —
/// there is no failure to log either, the way `merge_dependency::probe`
/// answers `Clear` rather than asking dropr about a task id that does not
/// exist (dropr:531).
pub(super) fn note_on_task(entry: &mut LedgerEntry, reason: &str) {
    let Some(task_id) = entry.dropr_task_id.clone() else {
        return;
    };
    let content = format!(
        "Overseer could not merge {} and handed the failure back to worker `{}` (handback {}): {reason}",
        entry.pr_url.as_deref().unwrap_or("the pull request"),
        entry.agent_id,
        entry.merge_recovery.charged
    );
    let repo_url = crate::overseer::repo_lookup::repo_url_for(&entry.repo);
    if let Err(error) = crate::dropr::scribble_create_timeout(
        &task_id,
        repo_url.as_deref(),
        &content,
        COMMAND_TIMEOUT,
    ) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overseer::ledger::LedgerPhase;

    fn entry() -> LedgerEntry {
        LedgerEntry {
            task_id: "agent-1".into(),
            dropr_task_id: None,
            display_id: "agent-1".into(),
            repo: "/repo".into(),
            agent_id: "agent-1".into(),
            branch: "branch".into(),
            phase: LedgerPhase::PrOpened,
            dispatched_at: chrono::Utc::now(),
            settled_at: None,
            retries: 0,
            pr_url: None,
            branch_updates: 0,
            merge_recovery: Default::default(),
            merge_hold: Default::default(),
            merge_hold_cap_escalated: false,
            merge_hold_rechecks: 0,
            merge_hold_recheck_reason: None,
            merge_hold_recheck_head: None,
            prerequisite_wait: None,
            merge_hold_stuck_notified: false,
            escalation_notified_reason: None,
            escalation_notified_head: None,
            worker_escalated: false,
            operator_override: None,
            merge_approval: None,
            pr_facts: None,
            worker_finished_at: None,
            approval_dropped: None,
            branch_update_head: None,
        }
    }

    /// An entry with no known dropr task — adopted from the registry, never
    /// dispatched through dropr — must not attempt the write at all. If this
    /// regressed to reading `entry.task_id` (an agent id) instead, this test
    /// would hang for `COMMAND_TIMEOUT` trying to reach dropr with a task id
    /// that was never real (dropr:531).
    #[test]
    fn an_entry_with_no_dropr_task_does_not_call_dropr() {
        let mut e = entry();
        note_on_task(&mut e, "worker too busy to take the handback");
    }
}
