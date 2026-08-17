//! Whether a dispatch pass needs an LLM judge round at all.
//!
//! The judge's authority is to reorder or omit ids the deterministic gate has
//! already approved. When every approved candidate fits the capacity still
//! available, "approve all of them" is the only outcome that authority can
//! produce, so the round buys nothing and costs an LLM call plus a whole extra
//! poll cycle of latency — the verdict is only consumed on the following pass.
//! Such a pass dispatches the gate's own ordering immediately instead.
//!
//! Note that today's gates cap approvals at capacity themselves (see
//! `candidate_skip`'s primary/secondary slot and `one_per_repo` rules), so
//! [`Route::Judged`] is reachable only if that ever changes. The check is
//! written against capacity rather than hard-coded to "always bypass" so the
//! judge comes back on its own the moment a contended pass can occur again.

use std::collections::BTreeSet;

use crate::overseer::{
    config::OverseerConfig,
    ledger::{ActiveWorkers, Ledger},
};

use super::Candidate;

/// How a pass reached its dispatch set. The label is written into
/// `decisions.jsonl` with every spawn, so an operator reading the log can tell
/// which dispatches an LLM had a hand in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Route {
    /// No judge round: the gate's own decision stands.
    Direct(&'static str),
    /// A judge round decides the order and membership of this pass.
    Judged,
}

impl Route {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Direct(reason) => reason,
            Self::Judged => "judge_approved",
        }
    }
}

pub(super) fn route(approved: usize, capacity: usize, judge_configured: bool) -> Route {
    if !judge_configured {
        // A daemon with no judge profile is a supported configuration, not a
        // degraded one: it must dispatch at any candidate count rather than
        // enqueue judgments nothing will ever answer.
        return Route::Direct("judge_unconfigured");
    }
    if approved <= capacity {
        return Route::Direct("judge_bypassed_uncontended");
    }
    Route::Judged
}

/// Worker slots this pass could still fill.
///
/// Capped at one per repository, mirroring `candidate_skip`'s own
/// one-new-worker-per-repository-per-pass stagger: extra slot headroom in a
/// repository cannot be spent in a single pass, however large
/// `parallel_limit` is. There is no cross-repository term any more — each
/// repository's primary/secondary slots are self-contained, so the number of
/// repositories with an approved candidate already bounds the total.
pub(super) fn remaining_capacity(
    config: &OverseerConfig,
    ledger: &Ledger,
    approved: &[Candidate],
) -> usize {
    let active = ledger.active_workers();
    approved
        .iter()
        .map(|candidate| candidate.repo.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|repo| repo_capacity(config, &active, repo))
        .sum()
}

/// Whether one more worker could dispatch into `repo` right now: the primary
/// slot when nothing is active there yet, a secondary slot when
/// `parallel_limit` still leaves room, and nothing otherwise. Mirrors
/// `candidate_skip`'s own slot check so the judge-routing estimate never
/// disagrees with what the real gate will do.
fn repo_capacity(config: &OverseerConfig, active: &ActiveWorkers, repo: &str) -> usize {
    let repo_active = active.repos.get(repo).copied().unwrap_or(0);
    if repo_active == 0 {
        return 1;
    }
    if config.parallel_limit == 0 {
        return 0;
    }
    usize::from(repo_active <= config.parallel_limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overseer::ledger::{LedgerEntry, LedgerPhase};
    use chrono::Utc;

    fn candidate(repo: &str) -> Candidate {
        Candidate {
            task_id: format!("task{repo}"),
            display_id: "#1".into(),
            title: "task".into(),
            repo: repo.into(),
            author: "allowed".into(),
            priority: "medium".into(),
            workspace: "workspace-1".into(),
            priority_score: None,
            status: "open".into(),
            parent_task_id: None,
        }
    }

    fn entry(repo: &str, agent_id: &str) -> LedgerEntry {
        LedgerEntry {
            task_id: "live".into(),
            display_id: "#0".into(),
            repo: repo.into(),
            agent_id: agent_id.into(),
            branch: "branch".into(),
            phase: LedgerPhase::Working,
            dispatched_at: Utc::now(),
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
    fn an_uncontended_pass_never_calls_the_judge() {
        assert_eq!(
            route(2, 3, true),
            Route::Direct("judge_bypassed_uncontended")
        );
        assert_eq!(
            route(3, 3, true),
            Route::Direct("judge_bypassed_uncontended")
        );
        assert_eq!(
            route(0, 0, true),
            Route::Direct("judge_bypassed_uncontended")
        );
    }

    #[test]
    fn a_contended_pass_still_calls_the_judge() {
        assert_eq!(route(4, 3, true), Route::Judged);
    }

    #[test]
    fn without_a_judge_profile_every_pass_dispatches_directly() {
        assert_eq!(route(9, 0, false), Route::Direct("judge_unconfigured"));
    }

    #[test]
    fn every_route_is_named_distinctly_in_the_decision_log() {
        // `spawn_candidate` writes this label with the dispatch, so an operator
        // reading `decisions.jsonl` can tell which dispatches an LLM touched.
        let labels = [
            route(1, 1, true).label(),
            route(9, 0, false).label(),
            route(4, 3, true).label(),
        ];
        assert_eq!(
            labels,
            [
                "judge_bypassed_uncontended",
                "judge_unconfigured",
                "judge_approved"
            ]
        );
    }

    #[test]
    fn a_manual_worker_still_spends_the_repositorys_primary_slot() {
        // A Manual worker still holds the branch and worktree the primary
        // slot accounts for, so it must count toward capacity exactly like
        // the real gate counts it — a human's worker is still occupancy.
        let config = OverseerConfig::default();
        let ledger = Ledger {
            entries: vec![entry("/one", "manual-agent"), entry("/two", "auto-agent")],
            ..Ledger::default()
        };

        let capacity =
            remaining_capacity(&config, &ledger, &[candidate("/one"), candidate("/two")]);
        assert_eq!(capacity, 0);
        assert_eq!(
            remaining_capacity(&config, &ledger, &[candidate("/three")]),
            1
        );
    }

    #[test]
    fn a_repository_at_its_primary_slot_contributes_no_capacity_without_parallel_limit() {
        let config = OverseerConfig::default();
        let ledger = Ledger {
            entries: vec![entry("/full", "auto-agent")],
            ..Ledger::default()
        };

        assert_eq!(
            remaining_capacity(&config, &ledger, &[candidate("/full")]),
            0
        );
        assert_eq!(
            remaining_capacity(&config, &ledger, &[candidate("/full"), candidate("/free")]),
            1
        );
    }

    #[test]
    fn a_parallel_limit_opens_secondary_capacity_in_the_same_repository() {
        let config = OverseerConfig {
            parallel_limit: 1,
            ..OverseerConfig::default()
        };
        let ledger = Ledger {
            entries: vec![entry("/full", "auto-agent")],
            ..Ledger::default()
        };

        assert_eq!(
            remaining_capacity(&config, &ledger, &[candidate("/full")]),
            1
        );
    }
}
