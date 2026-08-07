use std::collections::HashSet;

use super::terminal;
use crate::overseer::ledger::{Ledger, LedgerPhase};

/// Reason seeded on a killed-session escalation's reconsideration budget —
/// see `LedgerEntry::grant_merge_reconsideration`. Never a gate reason
/// itself, so the merge pass's first re-read of the pull request always
/// counts as a change against it.
const KILLED_SESSION: &str = "killed_session";

pub(crate) fn escalate_workers(ledger: &mut Ledger, killed_ids: &HashSet<String>) {
    for entry in &mut ledger.entries {
        if killed_ids.contains(&entry.agent_id) && !terminal(entry.phase) {
            entry.phase = LedgerPhase::Escalated;
            // A killed session is a forced intervention, not a worker's own
            // report — see `LedgerEntry::worker_escalated`. Unrelated activity
            // elsewhere is no evidence that whatever made the kill necessary
            // is resolved.
            entry.worker_escalated = false;
            // A killed session never goes through `merge_hold::charge`, so it
            // never earns a reconsideration budget the way a hold-cap
            // escalation does. Without one, a finished, green pull request
            // whose worker session died sits parked forever even after its
            // checks pass — see dropr:401. An entry with no pull request yet
            // has nothing the merge pass can read, so it stays parked
            // instead — re-dispatching it is the operator's call.
            if entry.pr_url.is_some() {
                entry.grant_merge_reconsideration(KILLED_SESSION);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::overseer::ledger::LedgerEntry;

    fn ledger_entry(agent_id: &str, phase: LedgerPhase) -> LedgerEntry {
        ledger_entry_with_pr(agent_id, phase, None)
    }

    fn ledger_entry_with_pr(
        agent_id: &str,
        phase: LedgerPhase,
        pr_url: Option<&str>,
    ) -> LedgerEntry {
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
            pr_url: pr_url.map(str::to_owned),
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

    #[test]
    fn escalate_workers_grants_reconsideration_when_a_pull_request_is_open() {
        let mut ledger = Ledger {
            entries: vec![ledger_entry_with_pr(
                "worker-1",
                LedgerPhase::PrOpened,
                Some("https://github.com/nantokaworks/robco/pull/177"),
            )],
            ..Ledger::default()
        };
        let killed_ids = HashSet::from(["worker-1".into()]);

        escalate_workers(&mut ledger, &killed_ids);

        let entry = &ledger.entries[0];
        assert_eq!(entry.phase, LedgerPhase::Escalated);
        assert!(entry.merge_hold_cap_escalated);
        assert_eq!(entry.merge_hold_rechecks, 0);
        assert_eq!(
            entry.merge_hold_recheck_reason.as_deref(),
            Some(KILLED_SESSION)
        );
    }

    #[test]
    fn escalate_workers_grants_no_reconsideration_without_a_pull_request() {
        let mut ledger = Ledger {
            entries: vec![ledger_entry("worker-1", LedgerPhase::Working)],
            ..Ledger::default()
        };
        let killed_ids = HashSet::from(["worker-1".into()]);

        escalate_workers(&mut ledger, &killed_ids);

        assert!(!ledger.entries[0].merge_hold_cap_escalated);
    }
}
