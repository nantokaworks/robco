use super::*;
use chrono::{TimeZone, Utc};
fn ledger() -> Ledger {
    Ledger {
        entries: vec![LedgerEntry {
            task_id: "task-131".into(),
            display_id: "#131".into(),
            repo: "/repo".into(),
            agent_id: "worker-1".into(),
            branch: "task-131".into(),
            phase: LedgerPhase::Dispatched,
            dispatched_at: Utc.with_ymd_and_hms(2026, 7, 16, 0, 0, 0).unwrap(),
            retries: 0,
            pr_url: None,
        }],
        ..Ledger::default()
    }
}
fn replay(lines: &[&str]) -> (Ledger, Vec<Action>) {
    let mut current = ledger();
    let mut actions = Vec::new();
    for line in lines {
        let snapshot: ObservationSnapshot = serde_json::from_str(line).unwrap();
        (current, actions) = reconcile(&current, &snapshot.observations, snapshot.at, 30);
    }
    (current, actions)
}
#[test]
fn replays_claimed_working_and_pr_opened_transitions() {
    let lines = [
        r#"{"at":"2026-07-16T00:01:00Z","observations":{"inbox":[{"at":"2026-07-16T00:01:00Z","agent_id":"worker-1","kind":"claimed","task_id":"task-131","pr_url":null,"reason":null}]}}"#,
        r#"{"at":"2026-07-16T00:02:00Z","observations":{"inbox":[{"at":"2026-07-16T00:02:00Z","agent_id":"worker-1","kind":"turn-done","task_id":"task-131","pr_url":null,"reason":null}]}}"#,
        r#"{"at":"2026-07-16T00:03:00Z","observations":{"inbox":[{"at":"2026-07-16T00:03:00Z","agent_id":"worker-1","kind":"done","task_id":"task-131","pr_url":"https://github.test/pull/1","reason":null}]}}"#,
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
#[test]
fn merged_snapshot_emits_cleanup_once_and_keeps_branch() {
    let line = r#"{"at":"2026-07-16T00:04:00Z","observations":{"prs":[{"taskId":"task-131","url":"https://github.test/pull/1","state":"MERGED","statusCheckRollup":[]}]}}"#;
    let (merged, actions) = replay(&[line]);
    assert_eq!(merged.entries[0].phase, LedgerPhase::Merged);
    assert!(actions.contains(&Action::KillSession {
        agent_id: "worker-1".into()
    }));
    assert!(actions.contains(&Action::RemoveWorktree {
        agent_id: "worker-1".into(),
        keep_branch: true,
    }));
    let (_, actions) = reconcile(
        &merged,
        &serde_json::from_str::<ObservationSnapshot>(line)
            .unwrap()
            .observations,
        Utc.with_ymd_and_hms(2026, 7, 16, 0, 5, 0).unwrap(),
        30,
    );
    assert!(actions.is_empty());
}
#[test]
fn merged_entry_reemits_cleanup_while_agent_is_registered() {
    let mut merged = ledger();
    merged.entries[0].phase = LedgerPhase::Merged;
    let observations: Observations =
        serde_json::from_str(r#"{"registered_agents":["worker-1"]}"#).unwrap();
    let (_, actions) = reconcile(
        &merged,
        &observations,
        Utc.with_ymd_and_hms(2026, 7, 16, 0, 5, 0).unwrap(),
        30,
    );
    assert!(actions.contains(&Action::KillSession {
        agent_id: "worker-1".into(),
    }));
    assert!(actions.contains(&Action::RemoveWorktree {
        agent_id: "worker-1".into(),
        keep_branch: true,
    }));
    let (_, actions) = reconcile(
        &merged,
        &Observations::default(),
        Utc.with_ymd_and_hms(2026, 7, 16, 0, 6, 0).unwrap(),
        30,
    );
    assert!(actions.is_empty());
}
#[test]
fn stuck_detection_uses_injected_now() {
    let observations: Observations = serde_json::from_str(r#"{"sessions":[{"agent_id":"worker-1","status":"running","last_activity_at":"2026-07-16T00:01:00Z"}]}"#).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 32, 0).unwrap();
    let (ledger, actions) = reconcile(&ledger(), &observations, now, 30);
    assert_eq!(ledger.entries[0].phase, LedgerPhase::Failed);
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, Action::MarkFailed { .. }))
    );
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
        reconcile(&claimed, &observations, now, 30).0.entries[0].phase,
        LedgerPhase::Claimed
    );
}
#[test]
fn open_task_escalates_only_after_claim() {
    let observations: Observations =
        serde_json::from_str(r#"{"tasks":[{"task_id":"task-131","state":"open"}]}"#).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 2, 0).unwrap();
    assert_eq!(
        reconcile(&ledger(), &observations, now, 30).0.entries[0].phase,
        LedgerPhase::Dispatched
    );
    let mut working = ledger();
    working.entries[0].phase = LedgerPhase::Working;
    assert_eq!(
        reconcile(&working, &observations, now, 30).0.entries[0].phase,
        LedgerPhase::Escalated
    );
}
#[test]
fn adopted_entry_heals_task_id_from_claimed_report() {
    let mut adopted = ledger();
    adopted.entries[0].task_id = "worker-1".into();
    let observations: Observations = serde_json::from_str(
        r#"{"inbox":[{"at":"2026-07-16T00:01:00Z","agent_id":"worker-1","kind":"claimed","task_id":"real-task-id","pr_url":null,"reason":null}]}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 2, 0).unwrap();
    let reconciled = reconcile(&adopted, &observations, now, 30).0;
    assert_eq!(reconciled.entries[0].task_id, "real-task-id");
    assert_eq!(reconciled.entries[0].phase, LedgerPhase::Claimed);
}
#[test]
fn delayed_done_does_not_revive_escalated_entry() {
    let mut escalated = ledger();
    escalated.entries[0].phase = LedgerPhase::Escalated;
    let observations: Observations = serde_json::from_str(
        r#"{"inbox":[{"at":"2026-07-16T00:03:00Z","agent_id":"worker-1","kind":"done","task_id":"task-131","pr_url":"https://github.test/pull/1","reason":null}]}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 4, 0).unwrap();
    assert_eq!(
        reconcile(&escalated, &observations, now, 30).0.entries[0].phase,
        LedgerPhase::Escalated
    );
}
#[test]
fn blocked_dead_and_unknown_observations_degrade_safely() {
    let malformed: Observations = serde_json::from_str(
        r#"{"inbox":[{"at":"2026-07-16T00:01:00Z","agent_id":"worker-1","kind":"future-kind","task_id":"task-131","pr_url":null,"reason":null}],"sessions":[{"agent_id":"worker-1","status":"future-status","last_activity_at":null}],"errors":["gh returned malformed JSON"]}"#,
    ).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 2, 0).unwrap();
    let (unchanged, actions) = reconcile(&ledger(), &malformed, now, 30);
    assert_eq!(unchanged.entries[0].phase, LedgerPhase::Dispatched);
    assert!(
        actions
            .iter()
            .filter(|a| matches!(a, Action::LogDecision { .. }))
            .count()
            >= 3
    );
    let blocked: Observations =
        serde_json::from_str(r#"{"inbox":[{"at":"2026-07-16T00:01:00Z","agent_id":"worker-1","kind":"blocked","task_id":"task-131","pr_url":null,"reason":"needs access"}]}"#)
            .unwrap();
    assert_eq!(
        reconcile(&ledger(), &blocked, now, 30).0.entries[0].phase,
        LedgerPhase::Escalated
    );
    let dead: Observations = serde_json::from_str(
        r#"{"sessions":[{"agent_id":"worker-1","status":"dead","last_activity_at":null}]}"#,
    )
    .unwrap();
    assert_eq!(
        reconcile(&ledger(), &dead, now, 30).0.entries[0].phase,
        LedgerPhase::Failed
    );
}
