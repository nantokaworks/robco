use std::collections::HashSet;

use super::terminal;
use crate::overseer::ledger::{Ledger, LedgerPhase};

pub(crate) fn escalate_workers(ledger: &mut Ledger, killed_ids: &HashSet<String>) {
    for entry in &mut ledger.entries {
        if killed_ids.contains(&entry.agent_id) && !terminal(entry.phase) {
            entry.phase = LedgerPhase::Escalated;
            // A killed session is a forced intervention, not a worker's own
            // report — see `LedgerEntry::worker_escalated`. Unrelated activity
            // elsewhere is no evidence that whatever made the kill necessary
            // is resolved.
            entry.worker_escalated = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::overseer::ledger::LedgerEntry;

    fn ledger_entry(agent_id: &str, phase: LedgerPhase) -> LedgerEntry {
        LedgerEntry {
            task_id: format!("task-{agent_id}"),
            display_id: "#177".into(),
            repo: "nantokaworks/robco".into(),
            agent_id: agent_id.into(),
            branch: "task-177".into(),
            phase,
            dispatched_at: Utc::now(),
            settled_at: None,
            retries: 0,
            pr_url: None,
            branch_updates: 0,
            merge_recovery: Default::default(),
            merge_hold: Default::default(),
            manual_merge_skip: None,
            merge_judge_fail_safes: 0,
            merge_hold_cap_escalated: false,
            merge_hold_rechecks: 0,
            merge_hold_recheck_reason: None,
            merge_hold_recheck_head: None,
            prerequisite_wait: None,
            merge_hold_stuck_notified: false,
            worker_escalated: false,
        }
    }

    #[test]
    fn escalate_workers_marks_matching_non_terminal_entries() {
        let mut ledger = Ledger {
            entries: vec![
                ledger_entry("worker-1", LedgerPhase::Dispatched),
                ledger_entry("worker-1", LedgerPhase::Claimed),
                ledger_entry("worker-1", LedgerPhase::Working),
                ledger_entry("worker-1", LedgerPhase::PrOpened),
            ],
            ..Ledger::default()
        };
        let killed_ids = HashSet::from(["worker-1".into()]);

        escalate_workers(&mut ledger, &killed_ids);

        assert!(
            ledger
                .entries
                .iter()
                .all(|entry| entry.phase == LedgerPhase::Escalated)
        );
    }

    #[test]
    fn escalate_workers_leaves_terminal_entries_unchanged() {
        let phases = [
            LedgerPhase::Merged,
            LedgerPhase::Failed,
            LedgerPhase::Escalated,
        ];
        let mut ledger = Ledger {
            entries: phases
                .iter()
                .copied()
                .map(|phase| ledger_entry("worker-1", phase))
                .collect(),
            ..Ledger::default()
        };
        let killed_ids = HashSet::from(["worker-1".into()]);

        escalate_workers(&mut ledger, &killed_ids);

        assert_eq!(
            ledger
                .entries
                .iter()
                .map(|entry| entry.phase)
                .collect::<Vec<_>>(),
            phases
        );
    }

    #[test]
    fn escalate_workers_leaves_unmatched_entries_unchanged() {
        let mut ledger = Ledger {
            entries: vec![ledger_entry("worker-2", LedgerPhase::Working)],
            ..Ledger::default()
        };
        let killed_ids = HashSet::from(["worker-1".into()]);

        escalate_workers(&mut ledger, &killed_ids);

        assert_eq!(ledger.entries[0].phase, LedgerPhase::Working);
    }
}
