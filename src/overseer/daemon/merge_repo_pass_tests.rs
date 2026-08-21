use super::*;
use crate::overseer::ledger::MergeApproval;

fn unrequested_entry(pr_url: Option<&str>) -> LedgerEntry {
    LedgerEntry {
        task_id: "task".into(),
        dropr_task_id: None,
        display_id: "#1".into(),
        repo: "/repo".into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
        phase: LedgerPhase::PrOpened,
        dispatched_at: chrono::Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: pr_url.map(str::to_owned),
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
    }
}

fn empty_registry() -> Registry {
    Registry {
        version: 1,
        repos: Vec::new(),
    }
}

// dropr:500 — a pull request nobody asked about is never gated.
#[test]
fn an_unrequested_pull_request_is_left_untouched_by_the_merge_pass() {
    let mut entry = unrequested_entry(Some("https://pr/1"));
    let before = entry.clone();
    let config = Config::default();
    let cache = ProtectionCache::default();
    let registry = empty_registry();
    let work = RepoWork {
        repo: "/repo".into(),
        entries: vec![&mut entry],
        settling: None,
    };

    let outcome = run(work, &config, &cache, &registry, 10, 5).unwrap();

    assert!(!outcome.merged);
    // No gate ran, no `gh` call happened, nothing about the entry changed —
    // in particular no decision was charged against its hold budget.
    assert_eq!(entry, before);
}

// dropr:500 — a merge the operator asked for that cannot proceed still
// escalates to the operator once its hold budget is spent. `pr_url: None`
// drives the gate's own `missing_pr_url` hold without shelling out to `gh`.
#[test]
fn a_requested_pull_request_that_cannot_proceed_escalates_once_its_hold_budget_is_spent() {
    let mut entry = unrequested_entry(None);
    entry.merge_approval = Some(MergeApproval {
        head: "abc123".into(),
        granted_at: chrono::Utc::now(),
    });
    let mut config = Config::default();
    config.overseer.max_merge_holds = 0;
    let cache = ProtectionCache::default();
    let registry = empty_registry();
    let work = RepoWork {
        repo: "/repo".into(),
        entries: vec![&mut entry],
        settling: None,
    };

    run(work, &config, &cache, &registry, 10, 5).unwrap();

    assert_eq!(entry.phase, LedgerPhase::Escalated);
}
