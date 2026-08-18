//! Dispatch capacity derived from the ledger: how many workers are live, and
//! which one holds each repository's primary slot. Split out of `ledger.rs`
//! per this project's file-size limit.

use std::collections::BTreeMap;

use super::{Ledger, LedgerEntry, holds_capacity};

/// Live workers counted globally and per repository.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct ActiveWorkers {
    pub count: usize,
    pub repos: BTreeMap<String, usize>,
}

impl Ledger {
    /// The workers occupying capacity right now. The dispatch gate and
    /// `robco overseer status` both read this one helper, so the count that
    /// enforces each repository's primary/secondary slots is the count the
    /// operator sees.
    ///
    /// Management mode is deliberately not a filter. Manual suppresses Overseer
    /// *intervention* — the worker belongs to a human, so it is never killed,
    /// restarted, or re-dispatched — but it still holds a worktree, a branch, a
    /// tmux session, and CPU in its repository. Exempting it from the caps would
    /// let a mode toggle free a slot the resources never released.
    pub fn active_workers(&self) -> ActiveWorkers {
        let mut repos: BTreeMap<String, usize> = BTreeMap::new();
        let mut count = 0;
        for entry in self.entries.iter().filter(|entry| holds_capacity(entry)) {
            count += 1;
            *repos.entry(entry.repo.clone()).or_default() += 1;
        }
        ActiveWorkers { count, repos }
    }

    /// The `display_id` of the task holding each repository's primary
    /// dispatch slot — the live entry that has been running longest there,
    /// by `dispatched_at`. Every other live entry in the repository holds a
    /// secondary slot instead. This is derived after the fact from whichever
    /// entry is oldest, since the ledger does not tag entries primary or
    /// secondary at dispatch time; see `dispatch::gate::candidate_skip` for
    /// how the two tiers are enforced going forward.
    pub fn primary_holders(&self) -> BTreeMap<String, String> {
        let mut holders: BTreeMap<String, &LedgerEntry> = BTreeMap::new();
        for entry in self.entries.iter().filter(|entry| holds_capacity(entry)) {
            holders
                .entry(entry.repo.clone())
                .and_modify(|current| {
                    if entry.dispatched_at < current.dispatched_at {
                        *current = entry;
                    }
                })
                .or_insert(entry);
        }
        holders
            .into_iter()
            .map(|(repo, entry)| (repo, entry.display_id.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::overseer::ledger::LedgerPhase;

    fn entry(repo: &str, display_id: &str, hour: u32) -> LedgerEntry {
        LedgerEntry {
            task_id: format!("task-{display_id}"),
            display_id: display_id.into(),
            repo: repo.into(),
            agent_id: "agent".into(),
            branch: "branch".into(),
            phase: LedgerPhase::Working,
            dispatched_at: Utc.with_ymd_and_hms(2026, 7, 16, hour, 0, 0).unwrap(),
            settled_at: None,
            retries: 0,
            pr_url: None,
            branch_updates: 0,
            merge_judge_primes: 0,
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
            escalation_notified_reason: None,
            escalation_notified_head: None,
            worker_escalated: false,
            operator_override: None,
            merge_approval: None,
        }
    }

    #[test]
    fn the_earliest_dispatched_live_entry_holds_the_primary_slot() {
        let ledger = Ledger {
            entries: vec![
                entry("/repo", "#2", 5),
                entry("/repo", "#1", 2),
                entry("/repo", "#3", 8),
            ],
            ..Ledger::default()
        };
        assert_eq!(
            ledger.primary_holders().get("/repo").map(String::as_str),
            Some("#1")
        );
    }

    #[test]
    fn a_repository_with_no_live_entries_has_no_primary_holder() {
        let mut settled = entry("/repo", "#1", 0);
        settled.phase = LedgerPhase::Merged;
        let ledger = Ledger {
            entries: vec![settled],
            ..Ledger::default()
        };
        assert!(ledger.primary_holders().is_empty());
    }
}
