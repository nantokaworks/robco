//! What lights the `merge-queued` state, and every way it stops (dropr:545).

use chrono::Utc;

use super::*;
use crate::{
    config::Config,
    overseer::ledger::{LedgerEntry, LedgerPhase, MergeApproval},
    registry::Registry,
};

const AGENT: &str = "worker-1";

fn app() -> App {
    let temp = tempfile::tempdir().unwrap();
    App::new(Registry::default(), Config::default(), temp.path().into())
}

fn entry(phase: LedgerPhase, approval: Option<MergeApproval>) -> LedgerEntry {
    LedgerEntry {
        task_id: "#545".into(),
        dropr_task_id: None,
        display_id: "#545".into(),
        repo: "nantokaworks/robco".into(),
        agent_id: AGENT.into(),
        branch: "robco/worker-1".into(),
        phase,
        dispatched_at: Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: None,
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
        merge_approval: approval,
        pr_facts: None,
        worker_finished_at: None,
        approval_dropped: None,
        branch_update_head: None,
    }
}

fn approval() -> MergeApproval {
    MergeApproval {
        head: "f002d389".into(),
        granted_at: Utc::now(),
    }
}

#[test]
fn nothing_is_queued_before_the_operator_presses_anything() {
    assert!(!app().merge_approval_queued(AGENT));
}

#[test]
fn a_queued_approval_shows_from_the_keypress_before_the_daemon_sees_it() {
    let mut app = app();
    app.note_merge_approval_queued(AGENT);
    assert!(app.merge_approval_queued(AGENT));
}

#[test]
fn one_agents_approval_says_nothing_about_another() {
    let mut app = app();
    app.note_merge_approval_queued(AGENT);
    assert!(!app.merge_approval_queued("worker-2"));
}

#[test]
fn the_ledger_keeps_it_lit_after_the_daemon_takes_the_approval() {
    let mut app = app();
    app.overseer_snapshot
        .ledger
        .entries
        .push(entry(LedgerPhase::PrOpened, Some(approval())));
    // The local record is gone, yet the badge must not blink at the hand-off.
    assert!(app.merge_approval_queued(AGENT));
    app.prune_queued_merge_approvals();
    assert!(app.merge_approval_queued(AGENT));
}

#[test]
fn a_dropped_approval_ends_it() {
    let mut app = app();
    app.note_merge_approval_queued(AGENT);
    // dropr:534: the daemon took the approval and then dropped it, so the
    // ledger no longer holds one. The row's own reason line says why.
    app.overseer_snapshot
        .ledger
        .entries
        .push(entry(LedgerPhase::PrOpened, None));
    app.queued_merge_approvals.clear();

    assert!(!app.merge_approval_queued(AGENT));
}

#[test]
fn a_stopped_entry_ends_it_even_while_the_local_record_is_young() {
    let mut app = app();
    app.note_merge_approval_queued(AGENT);
    app.overseer_snapshot
        .ledger
        .entries
        .push(entry(LedgerPhase::Merged, None));

    assert!(!app.merge_approval_queued(AGENT));
}

#[test]
fn a_merged_entry_that_still_carries_its_approval_is_not_lit() {
    let mut app = app();
    app.overseer_snapshot
        .ledger
        .entries
        .push(entry(LedgerPhase::Merged, Some(approval())));

    assert!(!app.merge_approval_queued(AGENT));
}

#[test]
fn a_record_the_daemon_never_took_ages_out() {
    let mut app = app();
    app.queued_merge_approvals.insert(
        AGENT.to_string(),
        QueuedApproval::aged(Duration::from_secs(60 * 60)),
    );

    assert!(!app.merge_approval_queued(AGENT));
}

#[test]
fn a_record_inside_the_bound_is_still_lit() {
    let mut app = app();
    app.overseer_snapshot.overseer.poll_interval_secs = 60;
    app.queued_merge_approvals.insert(
        AGENT.to_string(),
        QueuedApproval::aged(Duration::from_secs(120)),
    );

    assert!(app.merge_approval_queued(AGENT));
}

#[test]
fn a_very_short_poll_interval_still_gets_the_floor() {
    let mut app = app();
    app.overseer_snapshot.overseer.poll_interval_secs = 1;
    app.queued_merge_approvals.insert(
        AGENT.to_string(),
        QueuedApproval::aged(Duration::from_secs(20)),
    );

    assert!(app.merge_approval_queued(AGENT));
}

#[test]
fn pruning_drops_every_record_that_stopped_counting() {
    let mut app = app();
    app.note_merge_approval_queued("taken-over");
    app.note_merge_approval_queued("stopped");
    app.note_merge_approval_queued("still-waiting");
    app.queued_merge_approvals.insert(
        "aged-out".to_string(),
        QueuedApproval::aged(Duration::from_secs(60 * 60)),
    );
    let mut taken_over = entry(LedgerPhase::PrOpened, Some(approval()));
    taken_over.agent_id = "taken-over".into();
    let mut stopped = entry(LedgerPhase::Failed, None);
    stopped.agent_id = "stopped".into();
    app.overseer_snapshot.ledger.entries.push(taken_over);
    app.overseer_snapshot.ledger.entries.push(stopped);

    app.prune_queued_merge_approvals();

    let mut left: Vec<&String> = app.queued_merge_approvals.keys().collect();
    left.sort();
    assert_eq!(left, vec!["still-waiting"]);
}
