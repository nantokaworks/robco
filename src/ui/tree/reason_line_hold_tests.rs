//! The live-hold half of the reason line (dropr:529): a pull request still
//! open but held on something that will not clear on its own says so, without
//! reading as the stopped row dropr:524 introduced. The `merge_error` and
//! terminal-phase halves live in `reason_line_tests.rs` and
//! `reason_line_phase_tests.rs`; the fixtures all three use live in
//! `reason_line_test_support.rs`.

use crate::model::Selection;

use super::test_support::{
    app_with, clear_hold, drawn_rows, held_row, reason_row, rows, set_error, set_hold,
};

#[test]
fn checks_not_green_earns_a_line_on_the_first_pass() {
    let mut app = app_with(&["agt-a", "agt-b"]);
    set_hold(&mut app, "agt-a", "#529", "checks_not_green", 1);

    let rows = rows(&app);

    assert!(
        held_row(&rows)
            .expect("held line drawn")
            // Clipped by the pane, like any long reason (dropr:518).
            .contains("A required check failed"),
        "{rows:?}"
    );
    // Directly under the row it explains, and only one line gained.
    let agent_at = rows.iter().position(|row| row.contains("agt-a")).unwrap();
    assert!(rows[agent_at + 1].contains('⏸'), "{rows:?}");
    assert_eq!(drawn_rows(&rows), 5, "{rows:?}");
    // Never the stopped-row glyph — a hold has not stopped anything.
    assert!(reason_row(&rows).is_none(), "{rows:?}");
}

#[test]
fn checks_waiting_stays_quiet_on_a_freshly_opened_pull_request() {
    let mut app = app_with(&["agt-a"]);
    set_hold(&mut app, "agt-a", "#529", "checks_waiting", 1);

    let rows = rows(&app);

    assert!(
        held_row(&rows).is_none(),
        "checks still running is the system working, not a line: {rows:?}"
    );
    assert_eq!(drawn_rows(&rows), 3, "{rows:?}");
}

#[test]
fn checks_waiting_earns_a_line_once_it_has_survived_long_enough() {
    let mut app = app_with(&["agt-a"]);
    set_hold(&mut app, "agt-a", "#529", "checks_waiting", 3);

    let reason = held_row(&rows(&app)).expect("held line drawn");

    // Clipped by the pane, like any long reason (dropr:518).
    assert!(
        reason.contains("The pull request's checks are still"),
        "{reason:?}"
    );
}

#[test]
fn a_lifted_hold_removes_the_line_on_its_own() {
    let mut app = app_with(&["agt-a"]);
    set_hold(&mut app, "agt-a", "#529", "checks_not_green", 1);
    assert!(held_row(&rows(&app)).is_some());

    // The next auto-merge pass got past the gate — nothing the operator did.
    clear_hold(&mut app, "agt-a");

    assert!(
        held_row(&rows(&app)).is_none(),
        "the line must go when the hold does, with no operator action"
    );
}

/// The acceptance case for a row that is both held and failed: `merge_error`
/// is the operator's own merge attempt failing just now, which outranks a
/// hold the ledger may still be carrying from before. Only one line, and it
/// reads as the stopped row, not the held one.
#[test]
fn a_row_both_held_and_failed_shows_only_the_failure() {
    let mut app = app_with(&["agt-a"]);
    set_hold(&mut app, "agt-a", "#529", "checks_not_green", 1);
    set_error(&mut app, "agt-a", Some("merge refused"));

    let rows = rows(&app);

    assert_eq!(
        rows.iter().filter(|row| row.contains('⚠')).count(),
        1,
        "{rows:?}"
    );
    assert!(held_row(&rows).is_none(), "{rows:?}");
    let reason = reason_row(&rows).unwrap();
    assert!(reason.contains("merge refused"), "{reason:?}");
    assert_eq!(drawn_rows(&rows), 4, "one line gained, no more: {rows:?}");
}

/// The tree guides read the same for a held line as for a stopped one: it is
/// the same tail of the same row.
#[test]
fn a_held_lines_guide_matches_a_later_sibling() {
    let mut app = app_with(&["agt-a", "agt-b"]);
    set_hold(&mut app, "agt-a", "#529", "checks_not_green", 1);

    let rows = rows(&app);

    assert!(rows.iter().any(|row| row.starts_with("  ├── ")));
    let reason = held_row(&rows).unwrap();
    assert!(reason.starts_with("  │   ⏸ "), "{reason:?}");
}

/// The last sibling's held line leaves its own guide column blank, the same
/// way a stopped agent's does.
#[test]
fn a_held_lines_guide_matches_the_last_sibling() {
    let mut app = app_with(&["agt-a", "agt-b"]);
    set_hold(&mut app, "agt-b", "#529", "checks_not_green", 1);

    let rows = rows(&app);

    assert!(rows.iter().any(|row| row.starts_with("  └── ")));
    let reason = held_row(&rows).unwrap();
    assert!(!reason.starts_with("  │"), "{reason:?}");
    assert!(reason.starts_with("      ⏸ "), "{reason:?}");
}

/// The held line is not a `Selection`, so `j` steps from the held agent
/// straight to the next one.
#[test]
fn navigation_steps_over_the_held_line() {
    let mut app = app_with(&["agt-a", "agt-b"]);
    set_hold(&mut app, "agt-a", "#529", "checks_not_green", 1);
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

/// Selection is keyed by item, never by rendered line, so the cursor stays
/// put when a row gains or loses its held line.
#[test]
fn selection_survives_a_held_line_appearing_and_lifting() {
    let mut app = app_with(&["agt-a", "agt-b"]);
    app.selected = app
        .visible()
        .iter()
        .position(|item| matches!(item, Selection::Agent { agent: 1, .. }))
        .unwrap();
    let before = app.selected_item().map(|item| app.item_key(item));

    set_hold(&mut app, "agt-a", "#529", "checks_not_green", 1);
    app.restore_selection(before.clone());
    assert_eq!(app.selected_item().map(|item| app.item_key(item)), before);
    assert!(held_row(&rows(&app)).is_some());

    clear_hold(&mut app, "agt-a");
    app.restore_selection(before.clone());
    assert_eq!(app.selected_item().map(|item| app.item_key(item)), before);
}
