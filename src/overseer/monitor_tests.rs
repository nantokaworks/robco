use super::*;
use chrono::{TimeZone, Utc};
pub(super) fn ledger() -> Ledger {
    Ledger {
        entries: vec![LedgerEntry {
            task_id: "task-131".into(),
            display_id: "#131".into(),
            repo: "/repo".into(),
            agent_id: "worker-1".into(),
            branch: "task-131".into(),
            phase: LedgerPhase::Dispatched,
            dispatched_at: Utc.with_ymd_and_hms(2026, 7, 16, 0, 0, 0).unwrap(),
            settled_at: None,
            retries: 0,
            pr_url: None,
            branch_updates: 0,
            merge_judge_primes: 0,
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
            escalation_notified_reason: None,
            escalation_notified_head: None,
            worker_escalated: false,
            operator_override: None,
        }],
        ..Ledger::default()
    }
}
pub(super) fn replay(lines: &[&str]) -> (Ledger, Vec<Action>) {
    let mut current = ledger();
    let mut actions = Vec::new();
    for line in lines {
        let snapshot: ObservationSnapshot = serde_json::from_str(line).unwrap();
        (current, actions) = reconcile(&current, &snapshot.observations, snapshot.at, 30, 72);
    }
    (current, actions)
}
#[test]
fn replays_claimed_working_and_pr_opened_transitions() {
    let lines = [
        r#"{"at":"2026-07-16T00:01:00Z","observations":{"inbox":[{"at":"2026-07-16T00:01:00Z","agent_id":"worker-1","kind":"claimed"}]}}"#,
        r#"{"at":"2026-07-16T00:02:00Z","observations":{"inbox":[{"at":"2026-07-16T00:02:00Z","agent_id":"worker-1","kind":"turn-done"}]}}"#,
        r#"{"at":"2026-07-16T00:03:00Z","observations":{"prs":[{"taskId":"task-131","url":"https://github.test/pull/1","state":"OPEN","statusCheckRollup":[]}]}}"#,
    ];
    let (ledger, _) = replay(&lines[..1]);
    assert_eq!(ledger.entries[0].phase, LedgerPhase::Claimed);
    let (ledger, _) = replay(&lines[..2]);
    assert_eq!(ledger.entries[0].phase, LedgerPhase::Working);
    let (ledger, _) = replay(&lines);
    assert_eq!(ledger.entries[0].phase, LedgerPhase::PrOpened);
    assert_eq!(
        ledger.entries[0].pr_url.as_deref(),
        Some("https://github.test/pull/1")
    );
}

/// Pins the "Remove" side of the unreachable-arm decision (see dropr task
/// #315): a `done` inbox report never carries a PR url from its only
/// producer, so it is a no-op on the ledger — PR discovery happens
/// exclusively through the PR observation stream (see the test above).
#[test]
fn done_report_alone_does_not_advance_phase() {
    let observations: Observations = serde_json::from_str(
        r#"{"inbox":[{"at":"2026-07-16T00:01:00Z","agent_id":"worker-1","kind":"done"}]}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 2, 0).unwrap();
    let (result, actions) = reconcile(&ledger(), &observations, now, 30, 72);
    assert_eq!(result.entries[0].phase, LedgerPhase::Dispatched);
    assert!(result.entries[0].pr_url.is_none());
    assert!(actions.is_empty());
}
#[test]
fn stuck_detection_uses_injected_now() {
    let observations: Observations = serde_json::from_str(r#"{"sessions":[{"agent_id":"worker-1","status":"running","last_activity_at":"2026-07-16T00:01:00Z"}]}"#).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 32, 0).unwrap();
    let (ledger, actions) = reconcile(&ledger(), &observations, now, 30, 72);
    assert_eq!(ledger.entries[0].phase, LedgerPhase::Failed);
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, Action::MarkFailed { .. }))
    );
}

#[test]
fn manual_worker_session_death_does_not_fail_the_entry() {
    let observations: Observations = serde_json::from_str(
        r#"{"manual_agents":["worker-1"],"sessions":[{"agent_id":"worker-1","status":"dead","last_activity_at":null}]}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 1, 0, 0).unwrap();
    let (unchanged, actions) = reconcile(&ledger(), &observations, now, 30, 72);
    assert_eq!(unchanged.entries[0].phase, LedgerPhase::Dispatched);
    assert!(actions.is_empty());
}
#[test]
fn manual_worker_with_merged_pr_is_advanced_and_cleaned_up() {
    let observations: Observations = serde_json::from_str(
        r#"{"manual_agents":["worker-1"],"prs":[{"taskId":"task-131","url":"https://github.test/pull/1","state":"MERGED","statusCheckRollup":[]}]}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 1, 0, 0).unwrap();
    let (merged, actions) = reconcile(&ledger(), &observations, now, 30, 72);
    assert_eq!(merged.entries[0].phase, LedgerPhase::Merged);
    assert_eq!(
        merged.entries[0].pr_url.as_deref(),
        Some("https://github.test/pull/1")
    );
    assert!(actions.contains(&Action::KillSession {
        agent_id: "worker-1".into()
    }));
    assert!(actions.contains(&Action::RemoveWorktree {
        agent_id: "worker-1".into(),
    }));
    // Cleanup is re-emitted only while the registry row survives, so a merged
    // Manual entry does not re-kill an already-cleaned agent every poll.
    let (_, actions) = reconcile(&merged, &observations, now, 30, 72);
    assert!(actions.is_empty());
    let registered: Observations =
        serde_json::from_str(r#"{"manual_agents":["worker-1"],"registered_agents":["worker-1"]}"#)
            .unwrap();
    assert_eq!(reconcile(&merged, &registered, now, 30, 72).1.len(), 2);
}
/// Detaching a Manual worker ends Overseer ownership, so its entry leaves the
/// ledger instead of freezing there — the slot it held is released and the
/// running worker is left alone.
#[test]
fn detached_worker_entry_is_dropped_without_touching_the_worker() {
    let observations: Observations = serde_json::from_str(
        r#"{"manual_agents":["worker-1"],"detached_agents":["worker-1"],"sessions":[{"agent_id":"worker-1","status":"dead","last_activity_at":null}]}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 1, 0, 0).unwrap();
    let (dropped, actions) = reconcile(&ledger(), &observations, now, 30, 72);
    assert!(dropped.entries.is_empty());
    assert_eq!(dropped.active_workers().count, 0);
    // The drop is the only thing the pass does: the dead session it was told
    // about neither fails the entry nor kills anything.
    assert_eq!(actions.len(), 1);
    assert!(matches!(
        &actions[0],
        Action::LogDecision { task_id: Some(task_id), message }
            if task_id == "task-131" && message.contains("detached")
    ));
}

/// A merged entry is dropped too rather than cleaned up: the cleanup kills the
/// session and removes the worktree, which a detach explicitly does not do.
#[test]
fn detached_merged_entry_is_dropped_without_cleanup() {
    let mut merged = ledger();
    merged.entries[0].phase = LedgerPhase::Merged;
    let observations: Observations = serde_json::from_str(
        r#"{"detached_agents":["worker-1"],"registered_agents":["worker-1"]}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 1, 0, 0).unwrap();
    let (dropped, actions) = reconcile(&merged, &observations, now, 30, 72);
    assert!(dropped.entries.is_empty());
    assert!(!actions.contains(&Action::KillSession {
        agent_id: "worker-1".into(),
    }));
    assert!(!actions.contains(&Action::RemoveWorktree {
        agent_id: "worker-1".into(),
    }));
}
#[test]
fn manual_worker_with_open_pr_keeps_session_and_worktree() {
    let observations: Observations = serde_json::from_str(
        r#"{"manual_agents":["worker-1"],"prs":[{"taskId":"task-131","url":"https://github.test/pull/1","state":"OPEN","statusCheckRollup":[]}],"sessions":[{"agent_id":"worker-1","status":"dead","last_activity_at":null}],"tasks":[{"task_id":"task-131","state":"open"}],"inbox":[{"at":"2026-07-16T00:01:00Z","agent_id":"worker-1","kind":"blocked"}]}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 1, 0, 0).unwrap();
    let (open, actions) = reconcile(&ledger(), &observations, now, 30, 72);
    assert_eq!(open.entries[0].phase, LedgerPhase::PrOpened);
    assert!(actions.is_empty());
}
/// The history view orders entries by when they settled, so the instant a
/// worker phase becomes terminal has to be recorded — and recorded from the
/// pass's clock, which is the only one a test can pin down.
#[test]
fn reaching_a_terminal_phase_stamps_the_pass_clock() {
    let dead: Observations = serde_json::from_str(
        r#"{"sessions":[{"agent_id":"worker-1","status":"dead","last_activity_at":null}]}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 5, 0).unwrap();
    let (failed, _) = reconcile(&ledger(), &dead, now, 30, 72);
    assert_eq!(failed.entries[0].phase, LedgerPhase::Failed);
    assert_eq!(failed.entries[0].settled_at, Some(now));
}

/// The stamp is the moment the entry settled, not the moment it was last looked
/// at: re-running the pass an hour later must leave it alone, or the history
/// would report every terminal entry as having settled just now.
#[test]
fn a_later_pass_does_not_restamp_a_settled_entry() {
    let dead: Observations = serde_json::from_str(
        r#"{"sessions":[{"agent_id":"worker-1","status":"dead","last_activity_at":null}]}"#,
    )
    .unwrap();
    let settled = Utc.with_ymd_and_hms(2026, 7, 16, 0, 5, 0).unwrap();
    let (failed, _) = reconcile(&ledger(), &dead, settled, 30, 72);
    let later = Utc.with_ymd_and_hms(2026, 7, 16, 1, 5, 0).unwrap();
    let (again, _) = reconcile(&failed, &dead, later, 30, 72);
    assert_eq!(again.entries[0].settled_at, Some(settled));
}

/// A live entry has not settled, so it carries no timestamp for the history to
/// order it by.
#[test]
fn a_worker_phase_carries_no_settled_timestamp() {
    let observations: Observations = serde_json::from_str(
        r#"{"inbox":[{"at":"2026-07-16T00:01:00Z","agent_id":"worker-1","kind":"claimed"}]}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 2, 0).unwrap();
    let (claimed, _) = reconcile(&ledger(), &observations, now, 30, 72);
    assert_eq!(claimed.entries[0].phase, LedgerPhase::Claimed);
    assert!(claimed.entries[0].settled_at.is_none());
}

/// A Manual worker's entry advances through the same phases, so it settles the
/// same way — the human's merged pull request is history too.
#[test]
fn a_manual_entry_is_stamped_when_its_pull_request_merges() {
    let observations: Observations = serde_json::from_str(
        r#"{"manual_agents":["worker-1"],"prs":[{"taskId":"task-131","url":"https://github.test/pull/1","state":"MERGED","statusCheckRollup":[]}]}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 1, 0, 0).unwrap();
    let (merged, _) = reconcile(&ledger(), &observations, now, 30, 72);
    assert_eq!(merged.entries[0].settled_at, Some(now));
    let later = Utc.with_ymd_and_hms(2026, 7, 16, 2, 0, 0).unwrap();
    let (again, _) = reconcile(&merged, &observations, later, 30, 72);
    assert_eq!(again.entries[0].settled_at, Some(now));
}

#[test]
fn claimed_worker_without_activity_is_not_marked_stuck() {
    let mut claimed = ledger();
    claimed.entries[0].phase = LedgerPhase::Claimed;
    let observations: Observations = serde_json::from_str(
        r#"{"sessions":[{"agent_id":"worker-1","status":"running","last_activity_at":null}]}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 1, 0, 0).unwrap();
    assert_eq!(
        reconcile(&claimed, &observations, now, 30, 72).0.entries[0].phase,
        LedgerPhase::Claimed
    );
}
/// A missing `last_activity_at` on a still-`Dispatched` entry is a probe
/// fault reading (see `daemon::observations::tmux_activity`), not evidence
/// the worker has sat idle since it was spawned. Judging it against
/// `dispatched_at` used to fail an actively working worker on a single
/// transient probe miss — see dropr #440.
#[test]
fn dispatched_worker_without_activity_is_not_marked_stuck() {
    let observations: Observations = serde_json::from_str(
        r#"{"sessions":[{"agent_id":"worker-1","status":"running","last_activity_at":null}]}"#,
    )
    .unwrap();
    // Long past dispatched_at (2026-07-16T00:00:00Z) plus stuck_after_mins,
    // which is exactly the scenario the old dispatched_at fallback misjudged.
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 5, 0, 0).unwrap();
    assert_eq!(
        reconcile(&ledger(), &observations, now, 30, 72).0.entries[0].phase,
        LedgerPhase::Dispatched
    );
}
#[test]
fn open_task_escalates_only_after_claim() {
    let observations: Observations =
        serde_json::from_str(r#"{"tasks":[{"task_id":"task-131","state":"open"}]}"#).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 2, 0).unwrap();
    assert_eq!(
        reconcile(&ledger(), &observations, now, 30, 72).0.entries[0].phase,
        LedgerPhase::Dispatched
    );
    let mut working = ledger();
    working.entries[0].phase = LedgerPhase::Working;
    assert_eq!(
        reconcile(&working, &observations, now, 30, 72).0.entries[0].phase,
        LedgerPhase::Escalated
    );
}
#[test]
fn delayed_done_does_not_revive_escalated_entry() {
    let mut escalated = ledger();
    escalated.entries[0].phase = LedgerPhase::Escalated;
    let observations: Observations = serde_json::from_str(
        r#"{"inbox":[{"at":"2026-07-16T00:03:00Z","agent_id":"worker-1","kind":"done"}]}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 4, 0).unwrap();
    assert_eq!(
        reconcile(&escalated, &observations, now, 30, 72).0.entries[0].phase,
        LedgerPhase::Escalated
    );
}
#[test]
fn a_done_report_after_a_merge_escalation_requests_judgment_reconsideration() {
    let mut escalated = ledger();
    escalated.entries[0].phase = LedgerPhase::Escalated;
    escalated.entries[0].pr_url = Some("https://github.test/pull/1".into());
    escalated.entries[0].settled_at = Some(Utc.with_ymd_and_hms(2026, 7, 16, 0, 2, 0).unwrap());
    let observations: Observations = serde_json::from_str(
        r#"{"inbox":[{"at":"2026-07-16T00:03:00Z","agent_id":"worker-1","kind":"done"}]}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 4, 0).unwrap();
    let (result, actions) = reconcile(&escalated, &observations, now, 30, 72);
    // The judge, not this report, still decides whether the pull request
    // merges — so the phase stays put; only a fresh look is requested.
    assert_eq!(result.entries[0].phase, LedgerPhase::Escalated);
    assert!(actions.iter().any(|action| matches!(
        action,
        Action::ReconsiderMergeJudgment { task_id, pr_url }
            if task_id == "task-131" && pr_url == "https://github.test/pull/1"
    )));
}

#[test]
fn a_done_report_predating_the_escalation_does_not_request_reconsideration() {
    let mut escalated = ledger();
    escalated.entries[0].phase = LedgerPhase::Escalated;
    escalated.entries[0].pr_url = Some("https://github.test/pull/1".into());
    escalated.entries[0].settled_at = Some(Utc.with_ymd_and_hms(2026, 7, 16, 0, 5, 0).unwrap());
    let observations: Observations = serde_json::from_str(
        r#"{"inbox":[{"at":"2026-07-16T00:03:00Z","agent_id":"worker-1","kind":"done"}]}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 6, 0).unwrap();
    let (_, actions) = reconcile(&escalated, &observations, now, 30, 72);
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, Action::ReconsiderMergeJudgment { .. }))
    );
}

#[test]
fn a_done_report_after_escalation_with_no_pull_request_requests_nothing() {
    let mut escalated = ledger();
    escalated.entries[0].phase = LedgerPhase::Escalated;
    escalated.entries[0].settled_at = Some(Utc.with_ymd_and_hms(2026, 7, 16, 0, 2, 0).unwrap());
    let observations: Observations = serde_json::from_str(
        r#"{"inbox":[{"at":"2026-07-16T00:03:00Z","agent_id":"worker-1","kind":"done"}]}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 4, 0).unwrap();
    let (_, actions) = reconcile(&escalated, &observations, now, 30, 72);
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, Action::ReconsiderMergeJudgment { .. }))
    );
}

#[test]
fn blocked_dead_and_unknown_observations_degrade_safely() {
    let malformed: Observations = serde_json::from_str(
        r#"{"inbox":[{"at":"2026-07-16T00:01:00Z","agent_id":"worker-1","kind":"future-kind"}],"sessions":[{"agent_id":"worker-1","status":"future-status","last_activity_at":null}],"errors":[{"message":"gh returned malformed JSON","task_id":"task-131","repo":"/repo"}]}"#,
    ).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 2, 0).unwrap();
    let (unchanged, actions) = reconcile(&ledger(), &malformed, now, 30, 72);
    assert_eq!(unchanged.entries[0].phase, LedgerPhase::Dispatched);
    assert!(
        actions
            .iter()
            .filter(|a| matches!(a, Action::LogDecision { .. }))
            .count()
            >= 3
    );
    let blocked: Observations = serde_json::from_str(
        r#"{"inbox":[{"at":"2026-07-16T00:01:00Z","agent_id":"worker-1","kind":"blocked"}]}"#,
    )
    .unwrap();
    assert_eq!(
        reconcile(&ledger(), &blocked, now, 30, 72).0.entries[0].phase,
        LedgerPhase::Escalated
    );
    let dead: Observations = serde_json::from_str(
        r#"{"sessions":[{"agent_id":"worker-1","status":"dead","last_activity_at":null}]}"#,
    )
    .unwrap();
    assert_eq!(
        reconcile(&ledger(), &dead, now, 30, 72).0.entries[0].phase,
        LedgerPhase::Failed
    );
}

/// dropr:375 — a worker that discovers an ordering constraint reports
/// `waiting-prerequisite` instead of `blocked`. The wait is recorded, but
/// unlike `blocked` (see `blocked_dead_and_unknown_observations_degrade_safely`
/// above), the entry never escalates and no action fires — dropr's own ready
/// feed is what resolves this, not an operator.
#[test]
fn waiting_prerequisite_report_records_the_wait_without_escalating() {
    let observations: Observations = serde_json::from_str(
        r#"{"inbox":[{"at":"2026-07-16T00:01:00Z","agent_id":"worker-1","kind":"waiting-prerequisite"}]}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 2, 0).unwrap();
    let (waiting, actions) = reconcile(&ledger(), &observations, now, 30, 72);
    assert_eq!(waiting.entries[0].phase, LedgerPhase::Dispatched);
    assert_eq!(
        waiting.entries[0].prerequisite_wait,
        Some(Utc.with_ymd_and_hms(2026, 7, 16, 0, 1, 0).unwrap())
    );
    assert!(actions.is_empty());
}

/// The wait timestamp is set once, from the first report, so a redelivered or
/// repeated report cannot push the escalation bound further out.
#[test]
fn a_second_waiting_prerequisite_report_does_not_restart_the_clock() {
    let lines = [
        r#"{"at":"2026-07-16T00:01:00Z","observations":{"inbox":[{"at":"2026-07-16T00:01:00Z","agent_id":"worker-1","kind":"waiting-prerequisite"}]}}"#,
        r#"{"at":"2026-07-16T00:05:00Z","observations":{"inbox":[{"at":"2026-07-16T00:05:00Z","agent_id":"worker-1","kind":"waiting-prerequisite"}]}}"#,
    ];
    let (waiting, _) = replay(&lines);
    assert_eq!(
        waiting.entries[0].prerequisite_wait,
        Some(Utc.with_ymd_and_hms(2026, 7, 16, 0, 1, 0).unwrap())
    );
}

/// A worker waiting on a prerequisite occupies no dispatch capacity: its
/// own turn already ended, and dropr will not offer the task back until the
/// prerequisite closes — see `ledger::holds_capacity`.
#[test]
fn a_waiting_entry_does_not_count_toward_active_workers() {
    let observations: Observations = serde_json::from_str(
        r#"{"inbox":[{"at":"2026-07-16T00:01:00Z","agent_id":"worker-1","kind":"waiting-prerequisite"}]}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 2, 0).unwrap();
    let (waiting, _) = reconcile(&ledger(), &observations, now, 30, 72);
    assert_eq!(waiting.active_workers().count, 0);
}

/// The worker releases its dropr claim (`open`) as part of stepping aside for
/// the wait — `apply_task_failure`'s "worker released its dropr task lock"
/// escalation must not fire for a release the worker made on purpose.
#[test]
fn a_deliberate_claim_release_during_a_prerequisite_wait_does_not_escalate() {
    let observations: Observations = serde_json::from_str(
        r#"{"inbox":[{"at":"2026-07-16T00:01:00Z","agent_id":"worker-1","kind":"waiting-prerequisite"}],"tasks":[{"task_id":"task-131","state":"open"}]}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 2, 0).unwrap();
    let (waiting, actions) = reconcile(&ledger(), &observations, now, 30, 72);
    assert_eq!(waiting.entries[0].phase, LedgerPhase::Dispatched);
    assert!(actions.is_empty());
}

/// A worker that legitimately ended its turn to wait on a prerequisite must
/// not be reported "stuck" or "dead" for going quiet — see the guard in
/// `reconcile_entry` around `apply_session`.
#[test]
fn a_dead_session_while_waiting_on_a_prerequisite_is_not_treated_as_stuck() {
    let observations: Observations = serde_json::from_str(
        r#"{"inbox":[{"at":"2026-07-16T00:01:00Z","agent_id":"worker-1","kind":"waiting-prerequisite"}],"sessions":[{"agent_id":"worker-1","status":"dead","last_activity_at":null}]}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 2, 0).unwrap();
    let (waiting, actions) = reconcile(&ledger(), &observations, now, 30, 72);
    assert_eq!(waiting.entries[0].phase, LedgerPhase::Dispatched);
    assert!(actions.is_empty());
}

/// A prerequisite wait that outlives its bound escalates exactly once,
/// covering the never-landing or cyclical case the wait cannot otherwise
/// distinguish from a genuinely long-running but still-progressing one.
#[test]
fn a_prerequisite_wait_past_its_bound_escalates_once() {
    let mut waiting = ledger();
    waiting.entries[0].prerequisite_wait =
        Some(Utc.with_ymd_and_hms(2026, 7, 13, 0, 0, 0).unwrap());
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 1, 0, 0).unwrap();
    let (escalated, actions) = reconcile(&waiting, &Observations::default(), now, 30, 72);
    assert_eq!(escalated.entries[0].phase, LedgerPhase::Escalated);
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, Action::Escalate { .. }))
    );
    // Escalated is terminal, so a later pass leaves it alone rather than
    // re-escalating on every subsequent poll.
    let later = Utc.with_ymd_and_hms(2026, 7, 17, 1, 0, 0).unwrap();
    let (again, actions) = reconcile(&escalated, &Observations::default(), later, 30, 72);
    assert_eq!(again.entries[0].phase, LedgerPhase::Escalated);
    assert!(actions.is_empty());
}

/// Within the bound, nothing happens — a prerequisite wait is a steady state,
/// not a per-pass decision to record.
#[test]
fn a_prerequisite_wait_within_its_bound_is_silent() {
    let mut waiting = ledger();
    waiting.entries[0].prerequisite_wait =
        Some(Utc.with_ymd_and_hms(2026, 7, 16, 0, 0, 0).unwrap());
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 1, 0, 0).unwrap();
    let (still_waiting, actions) = reconcile(&waiting, &Observations::default(), now, 30, 72);
    assert_eq!(still_waiting.entries[0].phase, LedgerPhase::Dispatched);
    assert!(actions.is_empty());
}
