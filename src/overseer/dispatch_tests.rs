use chrono::{TimeZone, Utc};

use super::*;
use crate::model::ManagementMode;
use crate::overseer::ledger::{LedgerEntry, LedgerPhase};

fn candidate(repo: &str) -> Candidate {
    Candidate {
        task_id: "task-1".into(),
        display_id: "#1".into(),
        title: "task".into(),
        repo: repo.into(),
        author: "allowed".into(),
        priority: "medium".into(),
        workspace: "workspace-1".into(),
        priority_score: None,
    }
}

fn entry(phase: LedgerPhase) -> LedgerEntry {
    LedgerEntry {
        task_id: "old".into(),
        display_id: "#0".into(),
        repo: "/repo".into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
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
    }
}

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 16, 0, 0, 0).unwrap()
}

#[test]
fn circuit_opens_and_disables_dispatch() {
    let config = OverseerConfig {
        failure_circuit_threshold: 2,
        ..OverseerConfig::default()
    };
    let mut ledger = Ledger::default();
    ledger.counters.consecutive_failures = 2;
    let plan = plan_dispatch(
        &config,
        &ledger,
        &[candidate("/repo")],
        now(),
        &HashMap::new(),
    );
    assert!(plan.circuit_opened);
    assert!(!plan.dispatch_enabled);
    assert_eq!(plan.decisions[0].reason, "circuit_open");
}

#[test]
fn latched_circuit_reports_circuit_open_not_dispatch_disabled() {
    // Once `open_circuit` has persisted `dispatch_enabled = false`, every later
    // tick hits the disabled gate before the circuit branch. The reason must
    // stay `circuit_open` so the operator can tell the latched circuit apart
    // from an intentional manual disable.
    let reason_for = |threshold: u32, failures: u32| {
        let config = OverseerConfig {
            dispatch_enabled: false,
            failure_circuit_threshold: threshold,
            ..OverseerConfig::default()
        };
        let mut ledger = Ledger::default();
        ledger.counters.consecutive_failures = failures;
        let plan = plan_dispatch(
            &config,
            &ledger,
            &[candidate("/repo")],
            now(),
            &HashMap::new(),
        );
        assert!(!plan.decisions[0].dispatch);
        plan.decisions[0].reason.clone()
    };

    // Above and exactly at the threshold latch the circuit. Pinning the equality
    // boundary keeps a `>=` -> `>` regression from slipping through.
    assert_eq!(reason_for(3, 4), "circuit_open");
    assert_eq!(reason_for(3, 3), "circuit_open");
    // Just below the threshold is an operator-intended disable, not the circuit.
    assert_eq!(reason_for(3, 2), "dispatch_disabled");
    assert_eq!(reason_for(3, 0), "dispatch_disabled");
    // A zero threshold means the circuit is open the moment dispatch is disabled.
    assert_eq!(reason_for(0, 0), "circuit_open");
}

#[test]
fn daily_limit_and_date_reset() {
    let config = OverseerConfig {
        daily_dispatch_limit: 2,
        ..OverseerConfig::default()
    };
    let mut ledger = Ledger::default();
    ledger.counters.date = Some(now().date_naive());
    ledger.counters.dispatched_today = 2;
    assert_eq!(
        plan_dispatch(&config, &ledger, &[], now(), &HashMap::new()).decisions[0].reason,
        "daily_limit"
    );
    ledger.counters.date = Some(now().date_naive().pred_opt().unwrap());
    let plan = plan_dispatch(
        &config,
        &ledger,
        &[candidate("/repo")],
        now(),
        &HashMap::new(),
    );
    assert_eq!(plan.dispatched_today, 0);
    assert!(plan.decisions[0].dispatch);

    ledger.counters.date = Some(now().date_naive());
    ledger.counters.dispatched_today = 1;
    let plan = plan_dispatch(
        &config,
        &ledger,
        &[candidate("/one"), candidate("/two")],
        now(),
        &HashMap::new(),
    );
    assert!(plan.decisions[0].dispatch);
    assert_eq!(plan.decisions[1].reason, "daily_limit");
}

#[test]
fn zero_daily_limit_means_unlimited() {
    // 0 is the "no cap" sentinel: even a large dispatched_today must not trip the
    // daily_limit gate, at the global preflight or per-candidate stage.
    let config = OverseerConfig {
        daily_dispatch_limit: 0,
        ..OverseerConfig::default()
    };
    let mut ledger = Ledger::default();
    ledger.counters.date = Some(now().date_naive());
    ledger.counters.dispatched_today = 999;

    // Preflight (no candidates) must not short-circuit on daily_limit.
    let preflight = plan_dispatch(&config, &ledger, &[], now(), &HashMap::new());
    assert!(preflight.decisions.is_empty());

    // A candidate still gets dispatched despite dispatched_today far above any
    // positive limit.
    let plan = plan_dispatch(
        &config,
        &ledger,
        &[candidate("/repo")],
        now(),
        &HashMap::new(),
    );
    assert!(plan.decisions[0].dispatch);
    assert_eq!(plan.decisions[0].reason, "ready");
}

#[path = "dispatch_candidate_tests.rs"]
mod candidate_tests;
#[path = "dispatch_manual_tests.rs"]
mod manual_tests;
