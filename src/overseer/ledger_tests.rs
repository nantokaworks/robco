use super::*;

#[test]
fn save_load_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("overseer/ledger.json");
    let ledger = Ledger {
        entries: vec![LedgerEntry {
            task_id: "task-1".into(),
            display_id: "#128".into(),
            repo: "nantokaworks/robco".into(),
            agent_id: "worker-1".into(),
            branch: "task-128".into(),
            phase: LedgerPhase::PrOpened,
            dispatched_at: Utc::now(),
            settled_at: None,
            retries: 1,
            pr_url: Some("https://example.test/pr/1".into()),
            branch_updates: 0,
            merge_recovery: Default::default(),
            merge_hold: Default::default(),
            manual_merge_skip: None,
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
        }],
        skip_list: vec!["task-2".into()],
        counters: LedgerCounters {
            date: Some(Utc::now().date_naive()),
            dispatched_today: 2,
            consecutive_failures: 1,
        },
        merge_settling: [(
            "nantokaworks/robco".to_string(),
            MergeSettling { passes_held: 2 },
        )]
        .into_iter()
        .collect(),
        branch_exists_holds: [("task-4".to_string(), 3)].into_iter().collect(),
    };

    ledger.save_to(&path).unwrap();
    let serialized = fs::read_to_string(&path).unwrap();
    assert!(serialized.contains("\"pr_opened\""));
    assert_eq!(Ledger::load_from(&path).unwrap(), ledger);
}

/// A ledger written before the barrier existed still loads: the merge that was
/// in flight when the daemon was upgraded must not be turned into a corrupt
/// ledger and a lost board.
#[test]
fn a_ledger_without_the_settling_field_loads() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("ledger.json");
    fs::write(&path, r#"{"entries":[],"skip_list":[]}"#).unwrap();
    assert_eq!(Ledger::load_from(&path).unwrap(), Ledger::default());
}

#[test]
fn phases_serialize_to_required_strings() {
    let phases = [
        (LedgerPhase::Dispatched, "dispatched"),
        (LedgerPhase::Claimed, "claimed"),
        (LedgerPhase::Working, "working"),
        (LedgerPhase::PrOpened, "pr_opened"),
        (LedgerPhase::Merged, "merged"),
        (LedgerPhase::Failed, "failed"),
        (LedgerPhase::Escalated, "escalated"),
    ];

    for (phase, expected) in phases {
        assert_eq!(
            serde_json::to_string(&phase).unwrap(),
            format!("\"{expected}\"")
        );
    }
}

#[test]
fn active_workers_counts_every_non_terminal_entry() {
    let entry = |repo: &str, phase| LedgerEntry {
        task_id: "task-1".into(),
        display_id: "#1".into(),
        repo: repo.into(),
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
    };
    let ledger = Ledger {
        entries: vec![
            entry("/one", LedgerPhase::Working),
            entry("/one", LedgerPhase::PrOpened),
            entry("/two", LedgerPhase::Dispatched),
            entry("/one", LedgerPhase::Merged),
            entry("/two", LedgerPhase::Failed),
            entry("/three", LedgerPhase::Escalated),
        ],
        ..Ledger::default()
    };

    let active = ledger.active_workers();
    assert_eq!(active.count, 3);
    assert_eq!(
        active.repos,
        BTreeMap::from([("/one".to_string(), 2), ("/two".to_string(), 1)])
    );
    // The repository total is the sum of the per-repository counts, so the
    // global cap and the per-repository cap can never read different ledgers.
    assert_eq!(active.count, active.repos.values().sum::<usize>());
}

#[test]
fn a_ledger_written_before_merge_recovery_still_loads() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("ledger.json");
    fs::write(
        &path,
        r##"{"entries":[{"task_id":"t","display_id":"#1","repo":"/repo","agent_id":"a",
            "branch":"b","phase":"pr_opened","dispatched_at":"2026-07-01T00:00:00Z",
            "retries":0,"pr_url":null}],"skip_list":[],"counters":{}}"##,
    )
    .unwrap();
    let ledger = Ledger::load_from(&path).unwrap();
    let entry = &ledger.entries[0];
    assert_eq!(entry.branch_updates, 0);
    assert_eq!(entry.merge_recovery, MergeRecovery::default());
    // Nothing recorded when this entry settled, and nothing may be invented:
    // the history view reads the absence as "unknown", not as "just now".
    assert_eq!(entry.settled_at, None);
    // The merge in flight when the daemon was upgraded starts from a full hold
    // budget rather than from a ledger the new field cannot be read out of.
    assert_eq!(entry.merge_hold, MergeHold::default());
    // The file is intact, so the load was a genuine default rather than the
    // corrupt-ledger fallback.
    assert!(path.exists());
}

#[test]
fn manual_merge_skips_count_only_the_pull_requests_still_being_withheld() {
    let entry = |phase, skip: Option<&str>| LedgerEntry {
        task_id: "task-1".into(),
        display_id: "#1".into(),
        repo: "/one".into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
        phase,
        dispatched_at: Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: skip.map(str::to_owned),
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        manual_merge_skip: skip.map(str::to_owned),
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
    };
    let ledger = Ledger {
        entries: vec![
            entry(LedgerPhase::PrOpened, Some("https://pr/1")),
            entry(LedgerPhase::PrOpened, Some("https://pr/2")),
            entry(LedgerPhase::PrOpened, None),
            // Merged by its own owner: no longer held back by anyone.
            entry(LedgerPhase::Merged, Some("https://pr/3")),
        ],
        ..Ledger::default()
    };

    assert_eq!(ledger.manual_merge_skips(), 2);
}

#[test]
fn queued_merge_approvals_count_only_non_terminal_entries_with_a_live_approval() {
    let entry = |phase, approved: bool| LedgerEntry {
        task_id: "task-1".into(),
        display_id: "#1".into(),
        repo: "/one".into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
        phase,
        dispatched_at: Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: Some("https://pr/1".into()),
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        manual_merge_skip: None,
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
        merge_approval: approved.then(|| MergeApproval {
            head: "abc123".into(),
            granted_at: Utc::now(),
        }),
        pr_facts: None,
    };
    let ledger = Ledger {
        entries: vec![
            entry(LedgerPhase::PrOpened, true),
            entry(LedgerPhase::PrOpened, true),
            entry(LedgerPhase::PrOpened, false),
            // Already merged: its approval no longer describes anything pending.
            entry(LedgerPhase::Merged, true),
        ],
        ..Ledger::default()
    };

    assert_eq!(ledger.queued_merge_approvals(), 2);
}

#[test]
fn missing_ledger_defaults() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("ledger.json");
    assert_eq!(Ledger::load_from(&path).unwrap(), Ledger::default());
}

#[test]
fn corrupt_ledger_is_preserved_aside() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("ledger.json");
    fs::write(&path, "not json").unwrap();
    assert_eq!(Ledger::load_from(&path).unwrap(), Ledger::default());
    assert!(!path.exists());
    assert_eq!(
        fs::read_to_string(path.with_extension("json.corrupt")).unwrap(),
        "not json"
    );
}

#[test]
fn grant_merge_reconsideration_seeds_a_fresh_recheck_budget() {
    let mut entry = LedgerEntry {
        task_id: "task-1".into(),
        display_id: "#1".into(),
        repo: "/one".into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
        phase: LedgerPhase::Escalated,
        dispatched_at: Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: Some("https://pr/1".into()),
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        manual_merge_skip: None,
        merge_hold_cap_escalated: false,
        // A stale look from a prior escalation this entry has already
        // spent — the grant must reset it, not add to it.
        merge_hold_rechecks: 7,
        merge_hold_recheck_reason: Some("stale_reason".into()),
        merge_hold_recheck_head: Some("stale_head".into()),
        prerequisite_wait: None,
        merge_hold_stuck_notified: false,
        escalation_notified_reason: None,
        escalation_notified_head: None,
        worker_escalated: false,
        operator_override: None,
        merge_approval: None,
        pr_facts: None,
    };

    entry.grant_merge_reconsideration("killed_session");

    assert!(entry.merge_hold_cap_escalated);
    assert_eq!(entry.merge_hold_rechecks, 0);
    assert_eq!(
        entry.merge_hold_recheck_reason.as_deref(),
        Some("killed_session")
    );
    assert_eq!(entry.merge_hold_recheck_head, None);
}
