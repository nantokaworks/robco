use super::*;

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
