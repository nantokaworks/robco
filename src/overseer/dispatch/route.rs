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
//! `candidate_skip`'s `max_workers` / `per_repo_limit` / `one_per_repo` rules),
//! so [`Route::Judged`] is reachable only if that ever changes. The check is
//! written against capacity rather than hard-coded to "always bypass" so the
//! judge comes back on its own the moment a contended pass can occur again.

use std::collections::{BTreeSet, HashMap};

use crate::model::ManagementMode;
use crate::overseer::{config::OverseerConfig, ledger::Ledger};

use super::{Candidate, entries::worker_mode, terminal};

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
/// The global term counts Auto workers only — a Manual worker is a human's, and
/// the operator who started it did not thereby ask for a judge round. The
/// per-repository term is capped at one per repository because the gate admits
/// at most one new worker per repository per pass, so extra per-repository
/// headroom cannot be spent today however large `per_repo_limit` is.
pub(super) fn remaining_capacity(
    config: &OverseerConfig,
    ledger: &Ledger,
    approved: &[Candidate],
    worker_modes: &HashMap<String, ManagementMode>,
) -> usize {
    let active = ledger.active_workers();
    let auto = ledger
        .entries
        .iter()
        .filter(|entry| !terminal(entry.phase))
        .filter(|entry| worker_mode(entry, worker_modes) == ManagementMode::Auto)
        .count();
    let global = config.max_workers.saturating_sub(auto);
    let per_repo: usize = approved
        .iter()
        .map(|candidate| candidate.repo.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|repo| {
            config
                .per_repo_limit
                .saturating_sub(active.repos.get(repo).copied().unwrap_or(0))
                .min(1)
        })
        .sum();
    global.min(per_repo)
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
            operator_override: None,
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
    fn manual_workers_do_not_spend_judge_capacity() {
        // They do occupy a dispatch slot (the gate counts them), but a human's
        // worker is not a reason to ask an LLM which task to start next.
        let config = OverseerConfig {
            max_workers: 3,
            per_repo_limit: 2,
            ..OverseerConfig::default()
        };
        let ledger = Ledger {
            entries: vec![entry("/one", "manual-agent"), entry("/two", "auto-agent")],
            ..Ledger::default()
        };
        let modes = HashMap::from([
            ("manual-agent".to_string(), ManagementMode::Manual),
            ("auto-agent".to_string(), ManagementMode::Auto),
        ]);

        let capacity = remaining_capacity(
            &config,
            &ledger,
            &[candidate("/one"), candidate("/two")],
            &modes,
        );
        assert_eq!(capacity, 2);
    }

    #[test]
    fn a_repository_at_its_limit_contributes_no_capacity() {
        let config = OverseerConfig {
            max_workers: 5,
            per_repo_limit: 1,
            ..OverseerConfig::default()
        };
        let ledger = Ledger {
            entries: vec![entry("/full", "auto-agent")],
            ..Ledger::default()
        };
        let modes = HashMap::from([("auto-agent".to_string(), ManagementMode::Auto)]);

        assert_eq!(
            remaining_capacity(&config, &ledger, &[candidate("/full")], &modes),
            0
        );
        assert_eq!(
            remaining_capacity(
                &config,
                &ledger,
                &[candidate("/full"), candidate("/free")],
                &modes
            ),
            1
        );
    }
}
