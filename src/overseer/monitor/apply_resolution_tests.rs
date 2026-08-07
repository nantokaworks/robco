use chrono::TimeZone;

use super::*;
use crate::overseer::monitor::{BranchObservation, SessionObservation, TaskObservation};

fn at(minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 16, 0, minute, 0).unwrap()
}

fn escalated(settled_at: Option<DateTime<Utc>>) -> LedgerEntry {
    LedgerEntry {
        task_id: "task-131".into(),
        display_id: "#131".into(),
        repo: "/repo".into(),
        agent_id: "worker-1".into(),
        branch: "task-131".into(),
        phase: LedgerPhase::Escalated,
        dispatched_at: at(0),
        settled_at,
        retries: 0,
        pr_url: None,
        branch_updates: 0,
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
    }
}

fn resolved_reason(actions: &[Action]) -> Option<String> {
    actions.iter().find_map(|action| match action {
        Action::LogDecision {
            task_id: Some(task_id),
            message,
        } if task_id == "task-131" && message.starts_with("resolved_externally:") => {
            Some(message.clone())
        }
        _ => None,
    })
}

#[test]
fn tmux_activity_after_settling_lifts_the_escalation() {
    let mut entry = escalated(Some(at(5)));
    let observations = Observations {
        sessions: vec![SessionObservation {
            agent_id: "worker-1".into(),
            status: "running".into(),
            last_activity_at: Some(at(6)),
        }],
        ..Observations::default()
    };
    let mut actions = Vec::new();
    apply_escalation_resolution(&mut entry, &observations, &mut actions);

    assert_eq!(entry.phase, LedgerPhase::Working);
    assert_eq!(entry.settled_at, None);
    assert_eq!(
        resolved_reason(&actions).as_deref(),
        Some("resolved_externally:tmux_activity")
    );
}

/// The guard the task exists for: activity from before the escalation proves
/// nothing about whether the block itself lifted, so it must never resolve
/// the entry — only a signal timestamped strictly after `settled_at` may.
#[test]
fn activity_at_or_before_settling_never_resolves() {
    for last_activity_at in [at(5), at(4)] {
        let mut entry = escalated(Some(at(5)));
        let observations = Observations {
            sessions: vec![SessionObservation {
                agent_id: "worker-1".into(),
                status: "running".into(),
                last_activity_at: Some(last_activity_at),
            }],
            ..Observations::default()
        };
        let mut actions = Vec::new();
        apply_escalation_resolution(&mut entry, &observations, &mut actions);

        assert_eq!(entry.phase, LedgerPhase::Escalated);
        assert!(actions.is_empty());
    }
}

#[test]
fn a_new_commit_on_the_task_branch_lifts_the_escalation() {
    let mut entry = escalated(Some(at(5)));
    let observations = Observations {
        branches: vec![BranchObservation {
            task_id: "task-131".into(),
            latest_commit_at: Some(at(6)),
        }],
        ..Observations::default()
    };
    let mut actions = Vec::new();
    apply_escalation_resolution(&mut entry, &observations, &mut actions);

    assert_eq!(entry.phase, LedgerPhase::Working);
    assert_eq!(
        resolved_reason(&actions).as_deref(),
        Some("resolved_externally:new_commit")
    );
}

#[test]
fn a_dropr_task_update_after_settling_lifts_the_escalation() {
    let mut entry = escalated(Some(at(5)));
    let observations = Observations {
        tasks: vec![TaskObservation {
            task_id: "task-131".into(),
            state: "in_progress".into(),
            updated_at: Some(at(6)),
        }],
        ..Observations::default()
    };
    let mut actions = Vec::new();
    apply_escalation_resolution(&mut entry, &observations, &mut actions);

    assert_eq!(entry.phase, LedgerPhase::Working);
    assert_eq!(
        resolved_reason(&actions).as_deref(),
        Some("resolved_externally:dropr_task_updated")
    );
}

/// A legacy entry that escalated before `settled_at` existed has no baseline
/// to compare activity against, so it is left for a human exactly as it
/// always was — never silently resolved by a signal that might just as well
/// predate the escalation.
#[test]
fn no_settled_baseline_never_resolves() {
    let mut entry = escalated(None);
    let observations = Observations {
        sessions: vec![SessionObservation {
            agent_id: "worker-1".into(),
            status: "running".into(),
            last_activity_at: Some(at(50)),
        }],
        ..Observations::default()
    };
    let mut actions = Vec::new();
    apply_escalation_resolution(&mut entry, &observations, &mut actions);

    assert_eq!(entry.phase, LedgerPhase::Escalated);
    assert!(actions.is_empty());
}

#[test]
fn a_non_escalated_entry_is_left_alone() {
    let mut entry = escalated(Some(at(5)));
    entry.phase = LedgerPhase::Working;
    let observations = Observations {
        sessions: vec![SessionObservation {
            agent_id: "worker-1".into(),
            status: "running".into(),
            last_activity_at: Some(at(6)),
        }],
        ..Observations::default()
    };
    let mut actions = Vec::new();
    apply_escalation_resolution(&mut entry, &observations, &mut actions);

    assert_eq!(entry.phase, LedgerPhase::Working);
    assert!(actions.is_empty());
}
