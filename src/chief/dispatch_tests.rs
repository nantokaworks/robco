use chrono::{TimeZone, Utc};

use super::*;
use crate::chief::ledger::LedgerEntry;

fn candidate(repo: &str) -> Candidate {
    Candidate {
        task_id: "task-1".into(),
        display_id: "#1".into(),
        title: "task".into(),
        repo: repo.into(),
        author: "allowed".into(),
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
        retries: 0,
        pr_url: None,
    }
}

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 16, 0, 0, 0).unwrap()
}

#[test]
fn circuit_opens_and_disables_dispatch() {
    let config = ChiefConfig {
        failure_circuit_threshold: 2,
        ..ChiefConfig::default()
    };
    let mut ledger = Ledger::default();
    ledger.counters.consecutive_failures = 2;
    let plan = plan_dispatch(&config, &ledger, &[candidate("/repo")], now());
    assert!(plan.circuit_opened);
    assert!(!plan.dispatch_enabled);
    assert_eq!(plan.decisions[0].reason, "circuit_open");
}

#[test]
fn daily_limit_and_date_reset() {
    let config = ChiefConfig {
        daily_dispatch_limit: 2,
        ..ChiefConfig::default()
    };
    let mut ledger = Ledger::default();
    ledger.counters.date = Some(now().date_naive());
    ledger.counters.dispatched_today = 2;
    assert_eq!(
        plan_dispatch(&config, &ledger, &[], now()).decisions[0].reason,
        "daily_limit"
    );
    ledger.counters.date = Some(now().date_naive().pred_opt().unwrap());
    let plan = plan_dispatch(&config, &ledger, &[candidate("/repo")], now());
    assert_eq!(plan.dispatched_today, 0);
    assert!(plan.decisions[0].dispatch);

    ledger.counters.date = Some(now().date_naive());
    ledger.counters.dispatched_today = 1;
    let plan = plan_dispatch(
        &config,
        &ledger,
        &[candidate("/one"), candidate("/two")],
        now(),
    );
    assert!(plan.decisions[0].dispatch);
    assert_eq!(plan.decisions[1].reason, "daily_limit");
}

#[test]
fn candidate_filters_report_exact_reason() {
    struct Case {
        reason: &'static str,
        config: ChiefConfig,
        ledger: Ledger,
        candidate: Candidate,
    }
    let mut active = Ledger::default();
    active.entries.push(entry(LedgerPhase::Working));
    let mut skipped = Ledger::default();
    skipped.skip_list.push("task-1".into());
    let mut retried = Ledger::default();
    let mut failed = entry(LedgerPhase::Failed);
    failed.task_id = "task-1".into();
    failed.retries = 1;
    retried.entries.push(failed);
    let cases = vec![
        Case {
            reason: "per_repo_limit",
            config: ChiefConfig::default(),
            ledger: active.clone(),
            candidate: candidate("/repo"),
        },
        Case {
            reason: "max_workers",
            config: ChiefConfig {
                max_workers: 1,
                per_repo_limit: 2,
                ..ChiefConfig::default()
            },
            ledger: active,
            candidate: candidate("/other"),
        },
        Case {
            reason: "skip_list",
            config: ChiefConfig::default(),
            ledger: skipped,
            candidate: candidate("/repo"),
        },
        Case {
            reason: "max_retries",
            config: ChiefConfig::default(),
            ledger: retried,
            candidate: candidate("/repo"),
        },
        Case {
            reason: "author",
            config: ChiefConfig {
                dispatch_task_authors: vec!["someone-else".into()],
                ..ChiefConfig::default()
            },
            ledger: Ledger::default(),
            candidate: candidate("/repo"),
        },
    ];
    for case in cases {
        let plan = plan_dispatch(&case.config, &case.ledger, &[case.candidate], now());
        assert_eq!(plan.decisions[0].reason, case.reason);
        assert!(!plan.decisions[0].dispatch);
    }
}
