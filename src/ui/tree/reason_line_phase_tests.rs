//! The terminal-ledger-phase half of the reason line (dropr:524): a worker
//! whose ledger entry stopped in a phase the operator did not ask for says so
//! on its own row. The `merge_error` half lives in `reason_line_tests.rs`; the
//! fixtures both use live in `reason_line_test_support.rs`.

use crate::model::{Selection, Status};
use crate::overseer::ledger::LedgerPhase;
use crate::overseer::logging::DecisionKind;
use crate::ui::App;

use super::test_support::{
    app_with, drawn_rows, log_decision, log_phase_mirror, reason_row, rows, set_error, set_phase,
};

fn set_status(app: &mut App, name: &str, status: Status) {
    app.registry.repos[0]
        .agents
        .iter_mut()
        .find(|agent| agent.title == name)
        .expect("agent in fixture")
        .status = status;
}

#[test]
fn a_failed_worker_shows_why_it_failed() {
    let mut app = app_with(&["agt-a", "agt-b"]);
    set_phase(&mut app, "agt-a", "#524", LedgerPhase::Failed);
    log_decision(
        &mut app,
        DecisionKind::Hold,
        "#524",
        "worker exceeded stuck timeout",
    );

    let rows = rows(&app);

    assert!(
        reason_row(&rows)
            .expect("reason line drawn")
            .contains("worker exceeded stuck timeout"),
        "{rows:?}"
    );
    // Directly under the row it explains, and only one line gained.
    let agent_at = rows.iter().position(|row| row.contains("agt-a")).unwrap();
    assert!(rows[agent_at + 1].contains('⚠'), "{rows:?}");
    assert_eq!(drawn_rows(&rows), 5, "{rows:?}");
}

#[test]
fn an_escalated_worker_shows_why_it_escalated() {
    let mut app = app_with(&["agt-a"]);
    set_phase(&mut app, "agt-a", "#524", LedgerPhase::Escalated);
    log_decision(&mut app, DecisionKind::Escalate, "#524", "worker blocked");

    let reason = reason_row(&rows(&app)).expect("reason line drawn");

    assert!(reason.contains("worker blocked"), "{reason:?}");
}

/// A code-shaped reason reads as the same sentence the Inbox row shows for it,
/// so one condition is not two different strings depending on where you look.
#[test]
fn a_code_shaped_reason_reads_as_its_sentence() {
    let mut app = app_with(&["agt-a"]);
    set_phase(&mut app, "agt-a", "#524", LedgerPhase::Escalated);
    log_decision(
        &mut app,
        DecisionKind::Escalate,
        "#524",
        "merge_state:dirty",
    );

    let reason = reason_row(&rows(&app)).expect("reason line drawn");

    // The tail of the sentence is clipped by the pane, like any long reason.
    assert!(reason.contains("The branch has conflicts"), "{reason:?}");
    assert!(!reason.contains("merge_state:dirty"), "{reason:?}");
}

/// The decision `daemon::discord_events` writes to mirror the phase into
/// Discord lands *after* the one carrying the real reason, so a newest-wins
/// read has to skip it.
#[test]
fn the_phase_mirror_does_not_hide_the_real_reason() {
    let mut app = app_with(&["agt-a"]);
    set_phase(&mut app, "agt-a", "#524", LedgerPhase::Failed);
    log_decision(
        &mut app,
        DecisionKind::Hold,
        "#524",
        "worker session is dead",
    );
    log_phase_mirror(&mut app, DecisionKind::Hold, "#524", "task_failed");

    let reason = reason_row(&rows(&app)).expect("reason line drawn");

    assert!(reason.contains("worker session is dead"), "{reason:?}");
}

/// The snapshot holds only a tail of the log, so an old entry can have no
/// decision of its own left. The row still says it stopped.
#[test]
fn an_entry_with_no_decision_left_still_names_its_phase() {
    let mut app = app_with(&["agt-a"]);
    set_phase(&mut app, "agt-a", "#524", LedgerPhase::Failed);

    let reason = reason_row(&rows(&app)).expect("reason line drawn");

    assert!(
        reason.contains("The worker failed to finish this task."),
        "{reason:?}"
    );
}

/// `Merged` is terminal too, but it is the phase the work was dispatched to
/// reach — and every live phase is still in progress. None of them explain
/// anything, so a healthy tree keeps its height.
#[test]
fn a_phase_the_operator_asked_for_draws_no_line() {
    for phase in [
        LedgerPhase::Dispatched,
        LedgerPhase::Claimed,
        LedgerPhase::Working,
        LedgerPhase::PrOpened,
        LedgerPhase::Merged,
    ] {
        let mut app = app_with(&["agt-a"]);
        set_phase(&mut app, "agt-a", "#524", phase);
        log_decision(&mut app, DecisionKind::Hold, "#524", "checks_waiting");

        let rows = rows(&app);

        assert!(reason_row(&rows).is_none(), "{phase:?}: {rows:?}");
        assert_eq!(drawn_rows(&rows), 3, "{phase:?}: {rows:?}");
    }
}

/// The same gate the row's other ledger-sourced badges use: a worker that has
/// resumed real work has moved past what the ledger still records for it.
#[test]
fn a_worker_back_at_work_drops_the_line() {
    let mut app = app_with(&["agt-a"]);
    set_phase(&mut app, "agt-a", "#524", LedgerPhase::Escalated);
    log_decision(&mut app, DecisionKind::Escalate, "#524", "worker blocked");
    assert!(reason_row(&rows(&app)).is_some());

    set_status(&mut app, "agt-a", Status::Running);

    assert!(
        reason_row(&rows(&app)).is_none(),
        "a running worker's row must not carry the ledger's stale reason"
    );
}

/// A row qualifying twice still keeps a fixed height. `merge_error` is the
/// newer of the two by construction, so it wins — and the ledger's reason
/// takes over the moment it clears.
#[test]
fn a_row_that_qualifies_twice_draws_one_line() {
    let mut app = app_with(&["agt-a", "agt-b"]);
    set_phase(&mut app, "agt-a", "#524", LedgerPhase::Escalated);
    log_decision(&mut app, DecisionKind::Escalate, "#524", "worker blocked");
    set_error(&mut app, "agt-a", Some("branch has conflicts"));

    let drawn = rows(&app);

    assert_eq!(
        drawn.iter().filter(|row| row.contains('⚠')).count(),
        1,
        "{drawn:?}"
    );
    let reason = reason_row(&drawn).unwrap();
    assert!(reason.contains("branch has conflicts"), "{reason:?}");
    assert!(!reason.contains("worker blocked"), "{reason:?}");
    assert_eq!(drawn_rows(&drawn), 5, "{drawn:?}");

    // Pressing `m` again clears `merge_error`; the ledger's reason is what is
    // left to say.
    set_error(&mut app, "agt-a", None);

    let reason = reason_row(&rows(&app)).unwrap();
    assert!(reason.contains("worker blocked"), "{reason:?}");
}

/// The tree guides read the same for a ledger-sourced line as for a
/// `merge_error` one: it is the same tail of the same row.
#[test]
fn the_line_keeps_the_guide_of_the_row_it_explains() {
    let mut app = app_with(&["agt-a", "agt-b"]);
    set_phase(&mut app, "agt-a", "#524", LedgerPhase::Failed);
    log_decision(&mut app, DecisionKind::Hold, "#524", "worker is gone");

    let rows = rows(&app);

    assert!(rows.iter().any(|row| row.starts_with("  ├── ")));
    let reason = reason_row(&rows).unwrap();
    assert!(reason.starts_with("  │   ⚠ worker is gone"), "{reason:?}");
}

/// The line is not a `Selection`, whichever source drew it, so `j` steps from
/// the stopped agent straight to the next one.
#[test]
fn navigation_steps_over_a_ledger_sourced_line() {
    let mut app = app_with(&["agt-a", "agt-b"]);
    set_phase(&mut app, "agt-a", "#524", LedgerPhase::Failed);
    log_decision(&mut app, DecisionKind::Hold, "#524", "worker is gone");
    // The repo and its two agents — the extra rendered line adds no selection.
    assert_eq!(app.visible().len(), 3);

    app.selected = app
        .visible()
        .iter()
        .position(|item| matches!(item, Selection::Agent { agent: 0, .. }))
        .unwrap();
    app.move_selection_down();

    assert!(matches!(
        app.selected_item(),
        Some(Selection::Agent { agent: 1, .. })
    ));
}

/// Selection is keyed by item, never by rendered line, so the cursor stays put
/// when a row gains or loses a ledger-sourced line.
#[test]
fn selection_survives_a_ledger_sourced_line_appearing() {
    let mut app = app_with(&["agt-a", "agt-b"]);
    app.selected = app
        .visible()
        .iter()
        .position(|item| matches!(item, Selection::Agent { agent: 1, .. }))
        .unwrap();
    let before = app.selected_item().map(|item| app.item_key(item));

    set_phase(&mut app, "agt-a", "#524", LedgerPhase::Escalated);
    log_decision(&mut app, DecisionKind::Escalate, "#524", "worker blocked");
    app.restore_selection(before.clone());

    assert_eq!(app.selected_item().map(|item| app.item_key(item)), before);
    assert!(reason_row(&rows(&app)).is_some());
}
