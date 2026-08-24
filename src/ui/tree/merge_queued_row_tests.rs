//! The `merge-queued` badge as it actually lands on a tree row (dropr:545).
//!
//! `indicator::render` already pins the badge's text and its suppression
//! rule. What this file answers is the acceptance question: after the
//! operator presses `m` and the merge goes to the daemon, does the screen say
//! so — and does it stop saying so on every way that request can end?

use chrono::Utc;

use crate::{
    config::Config,
    overseer::ledger::{LedgerEntry, LedgerPhase, MergeApproval},
    registry::Registry,
    ui::{App, test_support},
};

use super::render_test_support::rendered_rows_at_width;

const BADGE: &str = "merge-queued";
const AGENT: &str = "agt-a";

/// One flat agent in one local repo, with the OVERSEER pane and host tmux
/// sessions out of the way so the rows are deterministic.
fn app() -> App {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::default();
    // Only worktrees under `worktree_root` survive `prune_unmanaged_agents`.
    let agent = test_support::agent(AGENT, config.worktree_root.join(AGENT));
    let registry = Registry {
        version: 1,
        repos: vec![test_support::repo(temp.path().join("repo"), vec![agent])],
    };
    let mut app = App::new(registry, config, temp.path().into());
    app.orphans = Vec::new();
    app.overseer_visible = false;
    app
}

fn entry(phase: LedgerPhase, approval: Option<MergeApproval>) -> LedgerEntry {
    LedgerEntry {
        task_id: "#545".into(),
        dropr_task_id: None,
        display_id: "#545".into(),
        repo: "nantokaworks/robco".into(),
        agent_id: AGENT.into(),
        branch: format!("robco/{AGENT}"),
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
    }
}

fn approval() -> MergeApproval {
    MergeApproval {
        head: "f002d389".into(),
        granted_at: Utc::now(),
    }
}

fn badge_shown(app: &App) -> bool {
    rendered_rows_at_width(app, 160)
        .iter()
        .any(|row| row.contains(BADGE))
}

/// The whole complaint in dropr:545: a healthy tree must look exactly as it
/// did before.
#[test]
fn a_tree_with_no_queued_merge_says_nothing_new() {
    assert!(!badge_shown(&app()));
}

/// Started. The daemon has not run yet, and the row already says robco has
/// the request.
#[test]
fn the_row_says_so_from_the_keypress() {
    let mut app = app();
    app.note_merge_approval_queued(AGENT);

    assert!(badge_shown(&app));
}

/// Ended normally, part one: the daemon took the approval. The badge stays,
/// now sourced from the ledger instead of the local record, so nothing
/// blinks at the hand-off.
#[test]
fn the_row_keeps_saying_so_once_the_daemon_has_it() {
    let mut app = app();
    app.note_merge_approval_queued(AGENT);
    app.overseer_snapshot
        .ledger
        .entries
        .push(entry(LedgerPhase::PrOpened, Some(approval())));
    app.prune_queued_merge_approvals();

    assert!(badge_shown(&app));
}

/// Ended normally, part two: the merge landed. Nothing is queued any more,
/// so the row stops saying it is.
#[test]
fn the_row_stops_once_the_merge_lands() {
    let mut app = app();
    app.note_merge_approval_queued(AGENT);
    app.overseer_snapshot
        .ledger
        .entries
        .push(entry(LedgerPhase::Merged, None));

    assert!(!badge_shown(&app));
}

/// Ended in failure, part one: dropr:534's dropped approval. The key the
/// operator pressed stopped counting, so the badge must stop too.
#[test]
fn the_row_stops_once_the_approval_is_dropped() {
    let mut app = app();
    let mut dropped = entry(LedgerPhase::PrOpened, None);
    dropped.approval_dropped = Some("merge_approval_dropped:stale_head:abc..def".into());
    app.overseer_snapshot.ledger.entries.push(dropped);

    assert!(!badge_shown(&app));
}

/// Ended in failure, part two: the entry escalated instead of merging.
#[test]
fn the_row_stops_once_the_entry_escalates() {
    let mut app = app();
    app.note_merge_approval_queued(AGENT);
    app.overseer_snapshot
        .ledger
        .entries
        .push(entry(LedgerPhase::Escalated, Some(approval())));

    assert!(!badge_shown(&app));
}
