use super::*;
use crate::overseer::ledger::{LedgerEntry, LedgerPhase};

/// dropr:473: `apply` only ever changes `ledger` in memory; `drain_in` used to
/// delete the request file right after `apply` returned, well before the
/// daemon's own end-of-pass save. A crash in that window lost both the
/// request and its effect. The applied mutation must now be durable on disk
/// — at `ledger_path`, independent of the caller ever saving `ledger` again —
/// before the file that caused it is acked.
#[test]
fn applied_request_is_checkpointed_to_disk_before_its_file_is_acked() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("requests");
    let ledger_path = temp.path().join("ledger.json");
    let mut ledger = Ledger {
        entries: vec![ledger_entry("worker-1", LedgerPhase::PrOpened)],
        ..Ledger::default()
    };
    enqueue_in(
        &dir,
        RuntimeRequest::PanicEscalate {
            source: "test".into(),
            agent_ids: vec!["worker-1".into()],
            at: Utc::now(),
        },
    )
    .unwrap();

    assert!(
        drain_in(&dir, &ledger_path, &mut ledger, None)
            .unwrap()
            .is_empty()
    );
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);

    let saved: Ledger = serde_json::from_str(&fs::read_to_string(&ledger_path).unwrap()).unwrap();
    assert_eq!(saved.entries[0].phase, LedgerPhase::Escalated);
}

/// When the checkpoint write itself cannot happen (here: `ledger_path`'s
/// parent is a file, not a directory, so `create_dir_all` fails), the request
/// file must be left in place rather than acked — an applied-but-unsaved
/// mutation must stay replayable on the next tick, not silently lost.
#[test]
fn checkpoint_failure_leaves_the_request_file_for_retry() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("requests");
    let blocked = temp.path().join("blocked");
    fs::write(&blocked, "not a directory").unwrap();
    let ledger_path = blocked.join("ledger.json");
    let mut ledger = Ledger {
        entries: vec![ledger_entry("worker-1", LedgerPhase::PrOpened)],
        ..Ledger::default()
    };
    enqueue_in(
        &dir,
        RuntimeRequest::PanicEscalate {
            source: "test".into(),
            agent_ids: vec!["worker-1".into()],
            at: Utc::now(),
        },
    )
    .unwrap();

    assert!(
        drain_in(&dir, &ledger_path, &mut ledger, None)
            .unwrap()
            .is_empty()
    );
    // Applied in memory (idempotent, so this is safe to have happened already)...
    assert_eq!(ledger.entries[0].phase, LedgerPhase::Escalated);
    // ...but not acked: the checkpoint never made it to disk.
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
}

#[test]
fn panic_escalate_marks_workers_including_pr_opened() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("requests");
    let ledger_path = temp.path().join("ledger.json");
    let mut ledger = Ledger {
        entries: vec![ledger_entry("worker-1", LedgerPhase::PrOpened)],
        ..Ledger::default()
    };
    enqueue_in(
        &dir,
        RuntimeRequest::PanicEscalate {
            source: "test".into(),
            agent_ids: vec!["worker-1".into()],
            at: Utc::now(),
        },
    )
    .unwrap();

    assert!(
        drain_in(&dir, &ledger_path, &mut ledger, None)
            .unwrap()
            .is_empty()
    );
    assert_eq!(ledger.entries[0].phase, LedgerPhase::Escalated);
}

#[test]
fn merge_completed_wakes_the_daemon_without_touching_state() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("requests");
    let ledger_path = temp.path().join("ledger.json");
    let mut ledger = Ledger {
        entries: vec![ledger_entry("worker-1", LedgerPhase::PrOpened)],
        ..Ledger::default()
    };
    ledger.counters.consecutive_failures = 2;
    enqueue_in(
        &dir,
        RuntimeRequest::MergeCompleted {
            source: "ui".into(),
            repo: "/repo".into(),
            at: Utc::now(),
        },
    )
    .unwrap();

    assert!(
        drain_in(&dir, &ledger_path, &mut ledger, None)
            .unwrap()
            .is_empty()
    );
    // The merge moves the ledger only once the daemon has observed it, so the
    // request itself leaves the phase and the failure streak alone.
    assert_eq!(ledger.entries[0].phase, LedgerPhase::PrOpened);
    assert_eq!(ledger.counters.consecutive_failures, 2);
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
}

#[test]
fn drain_missing_dir_is_noop() {
    let temp = tempfile::tempdir().unwrap();
    let ledger_path = temp.path().join("ledger.json");
    let mut ledger = Ledger::default();

    assert!(
        drain_in(
            &temp.path().join("missing"),
            &ledger_path,
            &mut ledger,
            None
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn corrupt_file_is_skipped() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("requests");
    let ledger_path = temp.path().join("ledger.json");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("bad.json"), "not json").unwrap();
    let mut ledger = Ledger::default();

    assert!(
        drain_in(&dir, &ledger_path, &mut ledger, None)
            .unwrap()
            .is_empty()
    );
    assert!(!dir.join("bad.json").exists());
    assert!(dir.join("bad.json.corrupt").exists());
}

#[test]
fn drain_applies_and_acks_all_pending_requests() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("requests");
    let ledger_path = temp.path().join("ledger.json");
    let mut ledger = Ledger {
        entries: vec![ledger_entry("worker-1", LedgerPhase::PrOpened)],
        ..Ledger::default()
    };
    enqueue_in(
        &dir,
        RuntimeRequest::PanicEscalate {
            source: "test".into(),
            agent_ids: vec!["worker-1".into()],
            at: Utc::now(),
        },
    )
    .unwrap();
    enqueue_in(
        &dir,
        RuntimeRequest::MergeCompleted {
            source: "test".into(),
            repo: "/repo".into(),
            at: Utc::now(),
        },
    )
    .unwrap();

    assert!(
        drain_in(&dir, &ledger_path, &mut ledger, None)
            .unwrap()
            .is_empty()
    );
    assert_eq!(ledger.entries[0].phase, LedgerPhase::Escalated);
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
}

#[test]
fn operator_merge_override_is_a_noop_when_no_entry_matches_the_target() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("requests");
    let ledger_path = temp.path().join("ledger.json");
    let mut ledger = Ledger {
        entries: vec![ledger_entry("worker-1", LedgerPhase::Escalated)],
        ..Ledger::default()
    };
    enqueue_in(
        &dir,
        RuntimeRequest::OperatorMergeOverride {
            source: "test".into(),
            target: "no-such-agent".into(),
            at: Utc::now(),
        },
    )
    .unwrap();

    assert!(
        drain_in(&dir, &ledger_path, &mut ledger, None)
            .unwrap()
            .is_empty()
    );
    assert!(ledger.entries[0].operator_override.is_none());
}

#[test]
fn operator_merge_override_is_a_noop_when_the_matched_entry_has_no_pull_request_yet() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("requests");
    let ledger_path = temp.path().join("ledger.json");
    let mut entry = ledger_entry("worker-1", LedgerPhase::Escalated);
    entry.pr_url = None;
    let mut ledger = Ledger {
        entries: vec![entry],
        ..Ledger::default()
    };
    enqueue_in(
        &dir,
        RuntimeRequest::OperatorMergeOverride {
            source: "test".into(),
            target: "worker-1".into(),
            at: Utc::now(),
        },
    )
    .unwrap();

    assert!(
        drain_in(&dir, &ledger_path, &mut ledger, None)
            .unwrap()
            .is_empty()
    );
    assert!(ledger.entries[0].operator_override.is_none());
}

#[test]
fn operator_merge_override_matches_by_display_id_but_leaves_no_override_when_the_pull_request_cannot_be_read()
 {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("requests");
    let ledger_path = temp.path().join("ledger.json");
    let mut entry = ledger_entry("worker-1", LedgerPhase::Escalated);
    // A repository path `gh` cannot even `chdir` into, so the read fails fast
    // and deterministically — no network or `gh` auth required for this test
    // to exercise the "matched, but GitHub could not be read" branch.
    entry.repo = "/no/such/repo/path".into();
    let mut ledger = Ledger {
        entries: vec![entry],
        ..Ledger::default()
    };
    enqueue_in(
        &dir,
        RuntimeRequest::OperatorMergeOverride {
            source: "test".into(),
            target: "#202".into(),
            at: Utc::now(),
        },
    )
    .unwrap();

    assert!(
        drain_in(&dir, &ledger_path, &mut ledger, None)
            .unwrap()
            .is_empty()
    );
    assert!(ledger.entries[0].operator_override.is_none());
}

/// dropr:470: a `RunTask` request (the TUI's cross-process equivalent of
/// Discord's `!run <task>`) must not be applied in-place — `apply` has no
/// `Config`/`now`/gathered candidates to dispatch with — it must come back
/// as a `PendingRun` for the caller to feed to `dispatch::run_named`, and the
/// file must still be acknowledged (removed) the same as any other request.
#[test]
fn run_task_is_handed_back_instead_of_applied_and_still_acked() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("requests");
    let ledger_path = temp.path().join("ledger.json");
    let mut ledger = Ledger::default();
    enqueue_in(
        &dir,
        RuntimeRequest::RunTask {
            source: "tui".into(),
            task: "#470".into(),
            at: Utc::now(),
        },
    )
    .unwrap();

    let pending_runs = drain_in(&dir, &ledger_path, &mut ledger, None).unwrap();
    assert_eq!(pending_runs.len(), 1);
    assert_eq!(pending_runs[0].task, "#470");
    assert_eq!(pending_runs[0].source, "tui");
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
}

#[test]
fn merge_approval_round_trips_through_json() {
    let request = RuntimeRequest::MergeApproval {
        source: "tui".into(),
        target: "worker-1".into(),
        head: "deadbeef".into(),
        at: Utc::now(),
    };
    let json = serde_json::to_string(&request).unwrap();
    assert_eq!(
        serde_json::from_str::<RuntimeRequest>(&json).unwrap(),
        request
    );
}

#[test]
fn merge_approval_replay_after_branch_moves_keeps_the_observed_head() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    std::process::Command::new("git")
        .args(["init", "-q", "-b", "task-202"])
        .current_dir(&repo)
        .status()
        .unwrap();
    fs::write(repo.join("tracked"), "one").unwrap();
    std::process::Command::new("git")
        .args(["add", "tracked"])
        .current_dir(&repo)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.test",
            "commit",
            "-q",
            "-m",
            "tip",
        ])
        .current_dir(&repo)
        .status()
        .unwrap();
    let approved_head = crate::git::local_branch_commit(&repo, "task-202").unwrap();
    let mut entry = ledger_entry("worker-1", LedgerPhase::Escalated);
    entry.repo = repo.display().to_string();
    entry.pr_url = None;
    let mut ledger = Ledger {
        entries: vec![entry],
        ..Ledger::default()
    };
    let request = RuntimeRequest::MergeApproval {
        source: "tui".into(),
        target: "worker-1".into(),
        head: approved_head.clone(),
        at: Utc::now(),
    };

    fs::write(repo.join("tracked"), "two").unwrap();
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.test",
            "commit",
            "-qam",
            "moved",
        ])
        .current_dir(&repo)
        .status()
        .unwrap();
    assert_ne!(
        crate::git::local_branch_commit(&repo, "task-202").unwrap(),
        approved_head
    );

    apply(&mut ledger, request.clone(), None);
    let first = ledger.entries[0].merge_approval.clone().unwrap();
    assert_eq!(first.head, approved_head);
    apply(&mut ledger, request, None);
    assert_eq!(ledger.entries[0].merge_approval.as_ref(), Some(&first));
}

fn ledger_entry(agent_id: &str, phase: LedgerPhase) -> LedgerEntry {
    LedgerEntry {
        task_id: format!("task-{agent_id}"),
        display_id: "#202".into(),
        repo: "nantokaworks/robco".into(),
        agent_id: agent_id.into(),
        branch: "task-202".into(),
        phase,
        dispatched_at: Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: Some("https://example.test/pr/202".into()),
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
