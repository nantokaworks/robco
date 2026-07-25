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
fn a_live_auto_worker_suppresses_redispatch() {
    // The task that opened the failure circuit: an Auto worker sitting in a
    // non-terminal phase kept its branch checked out while dispatch re-sent the
    // same task, so every re-spawn died in `git worktree add`.
    for phase in [
        LedgerPhase::Dispatched,
        LedgerPhase::Claimed,
        LedgerPhase::Working,
        LedgerPhase::PrOpened,
    ] {
        let mut ledger = Ledger::default();
        let mut live = entry(phase);
        live.task_id = "task-1".into();
        live.display_id = "#1".into();
        live.agent_id = "auto-agent".into();
        // A repository the candidate does not share, so per_repo_limit cannot be
        // what rejects it.
        live.repo = "/elsewhere".into();
        ledger.entries.push(live);
        let modes = HashMap::from([("auto-agent".to_string(), ManagementMode::Auto)]);

        let plan = plan_dispatch(
            &OverseerConfig::default(),
            &ledger,
            &[candidate("/repo")],
            now(),
            &modes,
        );
        assert_eq!(plan.decisions[0].reason, "active_worker");
        assert!(!plan.decisions[0].dispatch);
    }
}

#[test]
fn a_terminal_entry_does_not_suppress_redispatch() {
    // Only a live worker holds the branch. Once the entry reaches a terminal
    // phase the task is dispatchable again, bounded by max_retries_per_task.
    for phase in [
        LedgerPhase::Merged,
        LedgerPhase::Failed,
        LedgerPhase::Escalated,
    ] {
        let mut ledger = Ledger::default();
        let mut finished = entry(phase);
        finished.task_id = "task-1".into();
        finished.display_id = "#1".into();
        finished.repo = "/elsewhere".into();
        ledger.entries.push(finished);
        let config = OverseerConfig {
            max_retries_per_task: 2,
            ..OverseerConfig::default()
        };

        let plan = plan_dispatch(
            &config,
            &ledger,
            &[candidate("/repo")],
            now(),
            &HashMap::new(),
        );
        assert_eq!(plan.decisions[0].reason, "ready");
        assert!(plan.decisions[0].dispatch);
    }
}

#[test]
fn max_retries_bounds_attempts_per_task() {
    // `retries` records how many attempts preceded an entry, and every attempt
    // stamps that count onto the entries already tracking the task, so the ledger
    // states below are the ones a first, second, and third attempt leave behind.
    let attempt = |retries: u32| {
        let mut finished = entry(LedgerPhase::Failed);
        finished.task_id = "task-1".into();
        finished.display_id = "#1".into();
        finished.repo = "/elsewhere".into();
        finished.retries = retries;
        finished
    };
    let config = OverseerConfig {
        max_retries_per_task: 2,
        ..OverseerConfig::default()
    };
    let reason_after = |attempts: &[u32]| {
        let mut ledger = Ledger::default();
        ledger.entries.extend(attempts.iter().copied().map(attempt));
        let plan = plan_dispatch(
            &config,
            &ledger,
            &[candidate("/repo")],
            now(),
            &HashMap::new(),
        );
        plan.decisions[0].reason.clone()
    };

    assert_eq!(reason_after(&[0]), "ready");
    assert_eq!(reason_after(&[1, 1]), "ready");
    assert_eq!(reason_after(&[2, 2, 2]), "max_retries");
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
        ignored_fields: Vec::new(),
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
