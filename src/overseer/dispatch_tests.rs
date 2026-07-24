use chrono::{TimeZone, Utc};

use super::*;
use crate::model::ManagementMode;
use crate::overseer::ledger::LedgerEntry;

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

#[test]
fn candidate_filters_report_exact_reason() {
    struct Case {
        reason: &'static str,
        config: OverseerConfig,
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
            config: OverseerConfig::default(),
            ledger: active.clone(),
            candidate: candidate("/repo"),
        },
        Case {
            reason: "max_workers",
            config: OverseerConfig {
                max_workers: 1,
                per_repo_limit: 2,
                ..OverseerConfig::default()
            },
            ledger: active,
            candidate: candidate("/other"),
        },
        Case {
            reason: "skip_list",
            config: OverseerConfig::default(),
            ledger: skipped,
            candidate: candidate("/repo"),
        },
        Case {
            reason: "max_retries",
            config: OverseerConfig::default(),
            ledger: retried,
            candidate: candidate("/repo"),
        },
        Case {
            reason: "author",
            config: OverseerConfig {
                dispatch_task_authors: vec!["someone-else".into()],
                ..OverseerConfig::default()
            },
            ledger: Ledger::default(),
            candidate: candidate("/repo"),
        },
    ];
    for case in cases {
        let plan = plan_dispatch(
            &case.config,
            &case.ledger,
            &[case.candidate],
            now(),
            &HashMap::new(),
        );
        assert_eq!(plan.decisions[0].reason, case.reason);
        assert!(!plan.decisions[0].dispatch);
    }
}

#[test]
fn manual_worker_is_excluded_and_auto_worker_is_included() {
    let mut ledger = Ledger::default();
    let mut manual = entry(LedgerPhase::Failed);
    manual.task_id = "manual-task".into();
    manual.display_id = "#1".into();
    manual.agent_id = "manual-agent".into();
    let mut auto = entry(LedgerPhase::Failed);
    auto.task_id = "auto-task".into();
    auto.display_id = "#2".into();
    auto.agent_id = "auto-agent".into();
    ledger.entries.extend([manual, auto]);
    let modes = HashMap::from([
        ("manual-agent".into(), ManagementMode::Manual),
        ("auto-agent".into(), ManagementMode::Auto),
    ]);
    let candidates = [
        Candidate {
            task_id: "manual-task".into(),
            display_id: "#1".into(),
            title: "manual".into(),
            repo: "/manual".into(),
            author: "allowed".into(),
        },
        Candidate {
            task_id: "auto-task".into(),
            display_id: "#2".into(),
            title: "auto".into(),
            repo: "/auto".into(),
            author: "allowed".into(),
        },
    ];

    let plan = plan_dispatch(
        &OverseerConfig::default(),
        &ledger,
        &candidates,
        now(),
        &modes,
    );
    assert_eq!(plan.decisions[0].reason, "manual");
    assert!(!plan.decisions[0].dispatch);
    assert_eq!(plan.decisions[1].reason, "ready");
    assert!(plan.decisions[1].dispatch);
}

#[test]
fn judgment_cannot_add_a_candidate_rejected_by_rust_caps() {
    let config = OverseerConfig {
        max_workers: 1,
        ..OverseerConfig::default()
    };
    let mut first = candidate("/first");
    first.task_id = "first".into();
    let mut second = candidate("/second");
    second.task_id = "second".into();
    let candidates = [first, second];
    let plan = plan_dispatch(
        &config,
        &Ledger::default(),
        &candidates,
        now(),
        &HashMap::new(),
    );
    assert!(plan.decisions[0].dispatch);
    assert!(!plan.decisions[1].dispatch);
    let advice = DispatchAdvice {
        candidate_ids: vec![
            plan.decisions[1]
                .candidate
                .as_ref()
                .unwrap()
                .task_id
                .clone(),
        ],
        reason: "try rejected".into(),
        fail_safe: false,
    };
    let judged = apply_judgment(plan.decisions, &advice);
    assert!(!judged.iter().any(|decision| {
        decision.dispatch
            && decision
                .candidate
                .as_ref()
                .is_some_and(|item| item.repo == "/second")
    }));
}
