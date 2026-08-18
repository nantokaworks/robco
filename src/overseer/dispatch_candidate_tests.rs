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
    let mut doubly_active = Ledger::default();
    let mut secondary = entry(LedgerPhase::Working);
    secondary.task_id = "task-2".into();
    secondary.display_id = "#2".into();
    doubly_active
        .entries
        .extend([entry(LedgerPhase::Working), secondary]);
    let mut skipped = Ledger::default();
    skipped.skip_list.push("task-1".into());
    let mut retried = Ledger::default();
    let mut failed = entry(LedgerPhase::Failed);
    failed.task_id = "task-1".into();
    failed.retries = 1;
    retried.entries.push(failed);
    let cases = vec![
        Case {
            reason: "primary_slot_taken",
            config: OverseerConfig::default(),
            ledger: active,
            candidate: candidate("/repo"),
        },
        Case {
            reason: "parallel_slot_taken",
            config: OverseerConfig {
                parallel_limit: 1,
                ..OverseerConfig::default()
            },
            ledger: doubly_active,
            candidate: candidate("/repo"),
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
        Case {
            reason: "blocked",
            config: OverseerConfig::default(),
            ledger: Ledger::default(),
            candidate: Candidate {
                status: "blocked".into(),
                ..candidate("/repo")
            },
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
        // A repository the candidate does not share, so its primary slot cannot be
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
fn an_open_pull_request_names_a_specific_skip_reason() {
    // The loop that opened the failure circuit a second time: a worker finished,
    // pushed its branch, and opened a pull request, then its session ended. The
    // ledger entry stayed at `pr_opened` — non-terminal, so `active_worker`
    // would already hold it — but re-dispatching is not "wait for the worker",
    // it is "the operator's move is on the pull request", and the reason must
    // say so rather than reuse the generic label.
    let mut ledger = Ledger::default();
    let mut opened = entry(LedgerPhase::PrOpened);
    opened.task_id = "task-1".into();
    opened.display_id = "#1".into();
    opened.agent_id = "auto-agent".into();
    opened.repo = "/elsewhere".into();
    opened.pr_url = Some("https://github.com/example/repo/pull/717".into());
    ledger.entries.push(opened);
    let modes = HashMap::from([("auto-agent".to_string(), ManagementMode::Auto)]);

    let plan = plan_dispatch(
        &OverseerConfig::default(),
        &ledger,
        &[candidate("/repo")],
        now(),
        &modes,
    );
    assert_eq!(plan.decisions[0].reason, "pr_already_open");
    assert!(!plan.decisions[0].dispatch);
}

#[test]
fn a_closed_or_merged_pull_request_is_dispatchable_again() {
    // Once the entry that carried the pull request settles into a terminal
    // phase — merged, or escalated after closing unmerged — the task is not
    // waiting on anything any more and must be dispatchable, bounded only by
    // max_retries_per_task like any other finished attempt.
    for phase in [
        LedgerPhase::Merged,
        LedgerPhase::Failed,
        LedgerPhase::Escalated,
    ] {
        let mut ledger = Ledger::default();
        let mut settled = entry(phase);
        settled.task_id = "task-1".into();
        settled.display_id = "#1".into();
        settled.repo = "/elsewhere".into();
        settled.pr_url = Some("https://github.com/example/repo/pull/717".into());
        ledger.entries.push(settled);

        let plan = plan_dispatch(
            &OverseerConfig::default(),
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
fn reopening_a_blocked_task_makes_it_dispatchable_again() {
    // Self-clearing: flipping the status back to `open` (see the `blocked`
    // case above) is the entire unblock step, nothing else to reset.
    let reopened = candidate("/repo");
    assert_eq!(reopened.status, "open");
    let plan = plan_dispatch(
        &OverseerConfig::default(),
        &Ledger::default(),
        &[reopened],
        now(),
        &HashMap::new(),
    );
    assert_eq!(plan.decisions[0].reason, "ready");
    assert!(plan.decisions[0].dispatch);
}

/// dropr:452 acceptance: with `parallel_limit: 0` a second candidate in an
/// already-occupied repository holds on the primary slot alone; opening one
/// secondary slot lets exactly one more candidate through before the next
/// one holds on the secondary tier instead.
#[test]
fn parallel_limit_opens_exactly_that_many_secondary_slots() {
    let mut primary_active = Ledger::default();
    let mut primary = entry(LedgerPhase::Working);
    primary.task_id = "primary".into();
    primary.repo = "/repo".into();
    primary_active.entries.push(primary);

    // parallel_limit: 0 — the sole secondary candidate holds on the primary
    // slot, not the parallel one, because none exist to be taken.
    let serialized = plan_dispatch(
        &OverseerConfig::default(),
        &primary_active,
        &[candidate("/repo")],
        now(),
        &HashMap::new(),
    );
    assert_eq!(serialized.decisions[0].reason, "primary_slot_taken");
    assert!(!serialized.decisions[0].dispatch);

    // parallel_limit: 1 — one secondary candidate dispatches into the open
    // slot; a second one in the same pass finds it already spent.
    let config = OverseerConfig {
        parallel_limit: 1,
        ..OverseerConfig::default()
    };
    let mut second = candidate("/repo");
    second.task_id = "second".into();
    let mut third = candidate("/repo");
    third.task_id = "third".into();
    let parallel = plan_dispatch(
        &config,
        &primary_active,
        &[second, third],
        now(),
        &HashMap::new(),
    );
    assert_eq!(parallel.decisions[0].reason, "ready");
    assert!(parallel.decisions[0].dispatch);
    assert_eq!(parallel.decisions[1].reason, "parallel_slot_taken");
    assert!(!parallel.decisions[1].dispatch);
}

/// dropr:375 — a worker that stepped aside to wait on a dropr `blocks`
/// dependency edge is not "an active worker" any more: its entry stays at a
/// live phase (it never reached a terminal one), but `active_worker` must
/// not hold the task on its account, or the task could never be redispatched
/// once dropr's ready feed offers it again.
#[test]
fn a_prerequisite_wait_does_not_suppress_redispatch() {
    let mut ledger = Ledger::default();
    let mut waiting = entry(LedgerPhase::Working);
    waiting.task_id = "task-1".into();
    waiting.display_id = "#1".into();
    waiting.repo = "/elsewhere".into();
    waiting.prerequisite_wait = Some(now());
    ledger.entries.push(waiting);

    let plan = plan_dispatch(
        &OverseerConfig::default(),
        &ledger,
        &[candidate("/repo")],
        now(),
        &HashMap::new(),
    );
    assert_eq!(plan.decisions[0].reason, "ready");
    assert!(plan.decisions[0].dispatch);
}
