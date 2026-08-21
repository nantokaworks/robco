//! The `merge_error` half of the reason line (dropr:518): that it draws, what
//! it draws, and that the tree's guides and cursor survive it. The
//! terminal-ledger-phase half lives in `reason_line_phase_tests.rs`; the
//! fixtures both use live in `reason_line_test_support.rs`.

use crate::model::Selection;
use crate::ui::tree::render_test_support::{rendered_rows_at_width, title_column};

use super::test_support::{NARROW, app_from, app_with, drawn_rows, reason_row, rows, set_error};

#[test]
fn an_agent_without_an_error_stays_exactly_one_line() {
    let app = app_with(&["agt-a", "agt-b"]);

    let rows = rows(&app);

    assert!(
        reason_row(&rows).is_none(),
        "a healthy tree must not grow a second line: {rows:?}"
    );
    assert_eq!(
        drawn_rows(&rows),
        4,
        "PROJECTS header, the repo, and two agents: {rows:?}"
    );
}

#[test]
fn an_agent_with_a_merge_error_gains_a_reason_line() {
    let mut app = app_with(&["agt-a", "agt-b"]);
    set_error(&mut app, "agt-a", Some("branch has conflicts"));

    let rows = rows(&app);

    assert!(
        reason_row(&rows)
            .expect("reason line drawn")
            .contains("branch has conflicts"),
        "{rows:?}"
    );
    // Directly under the row it explains, not at the end of the repo.
    let agent_at = rows.iter().position(|row| row.contains("agt-a")).unwrap();
    assert!(rows[agent_at + 1].contains('⚠'), "{rows:?}");
    assert_eq!(drawn_rows(&rows), 5, "one line gained, no more: {rows:?}");
}

#[test]
fn the_reason_line_disappears_when_the_error_clears() {
    let mut app = app_with(&["agt-a", "agt-b"]);
    set_error(&mut app, "agt-a", Some("branch has conflicts"));
    assert!(reason_row(&rows(&app)).is_some());

    set_error(&mut app, "agt-a", None);

    assert!(
        reason_row(&rows(&app)).is_none(),
        "the line must go when the error does"
    );
}

#[test]
fn an_empty_error_draws_no_line() {
    let mut app = app_with(&["agt-a"]);
    set_error(&mut app, "agt-a", Some("  \n "));

    assert!(
        reason_row(&rows(&app)).is_none(),
        "an error with nothing to say says nothing"
    );
}

/// The reason starts in the agent's own title column, so the line reads as
/// the tail of the row above it.
#[test]
fn the_reason_starts_in_the_agents_own_title_column() {
    let mut app = app_with(&["agt-a", "agt-b"]);
    set_error(&mut app, "agt-a", Some("conflicts"));

    let rows = rows(&app);

    assert_eq!(
        title_column(&rows, "conflicts"),
        title_column(&rows, "agt-a")
    );
}

/// An agent with a later sibling leaves a continuing `│` over its own column,
/// so the run down to that sibling is unbroken.
#[test]
fn a_reason_line_continues_the_guide_when_a_sibling_follows() {
    let mut app = app_with(&["agt-a", "agt-b"]);
    set_error(&mut app, "agt-a", Some("conflicts"));

    let rows = rows(&app);

    assert!(rows.iter().any(|row| row.starts_with("  ├── ")));
    let reason = reason_row(&rows).unwrap();
    assert!(reason.starts_with("  │   ⚠ conflicts"), "{reason:?}");
}

/// The last sibling has nothing below it at its own column, so the reason
/// line leaves that column blank rather than drawing a guide into nothing.
#[test]
fn the_last_siblings_reason_line_leaves_its_guide_column_blank() {
    let mut app = app_with(&["agt-a", "agt-b"]);
    set_error(&mut app, "agt-b", Some("conflicts"));

    let rows = rows(&app);

    assert!(rows.iter().any(|row| row.starts_with("  └── ")));
    let reason = reason_row(&rows).unwrap();
    assert!(!reason.starts_with("  │"), "{reason:?}");
    assert!(reason.starts_with("      ⚠ conflicts"), "{reason:?}");
}

/// A nested agent's reason line keeps every ancestor guide above it.
#[test]
fn a_nested_agents_reason_line_keeps_its_ancestor_guides() {
    // `agt-c` is `agt-a`'s only child; `agt-b` is `agt-a`'s later sibling, so
    // the guide over `agt-a`'s column must keep drawing beside both of them.
    let mut app = app_from(&["agt-a", "agt-b", "agt-c"], |index| {
        (index == 2).then(|| "agt-a".to_string())
    });
    set_error(&mut app, "agt-c", Some("conflicts"));

    let rows = rows(&app);

    assert!(
        rows.iter()
            .any(|row| row.starts_with("  │ └── ") && row.contains("agt-c"))
    );
    // `agt-c` is its parent's last child, so its own column stays blank — but
    // `agt-a`'s guide above it does not.
    let reason = reason_row(&rows).unwrap();
    assert!(reason.starts_with("  │     ⚠ conflicts"), "{reason:?}");
}

/// A failed agent keeps its children flush under the reason line: the line
/// draws the same column they do, so nothing shifts.
#[test]
fn a_failed_agent_with_children_keeps_them_flush_under_the_line() {
    let mut app = app_from(&["agt-a", "agt-b", "agt-c"], |index| {
        (index == 2).then(|| "agt-a".to_string())
    });
    set_error(&mut app, "agt-a", Some("conflicts"));

    let rows = rows(&app);

    // Parent, its reason line, then the child — in that order, with the same
    // guide column running through all three.
    let reason_at = rows.iter().position(|row| row.contains('⚠')).unwrap();
    assert!(rows[reason_at - 1].starts_with("  ├── "));
    assert!(rows[reason_at - 1].contains("agt-a"));
    assert!(rows[reason_at].starts_with("  │   ⚠ "));
    assert!(rows[reason_at + 1].starts_with("  │ └── "));
    assert!(rows[reason_at + 1].contains("agt-c"));
}

/// A tree row keeps a fixed height, so only the first line of a `gh`/`git`
/// dump reaches the tree.
#[test]
fn a_multi_line_error_shows_only_its_first_line() {
    let mut app = app_with(&["agt-a"]);
    let dump = "merge refused\nrun gh pr checks\nthen retry";
    set_error(&mut app, "agt-a", Some(dump));

    let rows = rows(&app);

    let reason = reason_row(&rows).unwrap();
    assert!(reason.contains("merge refused"), "{reason:?}");
    assert!(!reason.contains("run gh pr checks"), "{reason:?}");
    assert_eq!(rows.iter().filter(|row| row.contains('⚠')).count(), 1);
}

/// A line longer than the pane is clipped, never wrapped and never drawn
/// past the pane's own edge.
#[test]
fn a_long_error_is_clipped_to_the_pane_width() {
    let mut app = app_with(&["agt-a"]);
    let long = format!("conflict in {}", "src/very/long/path.rs ".repeat(12));
    set_error(&mut app, "agt-a", Some(&long));

    let rows = rendered_rows_at_width(&app, NARROW);

    let reason = reason_row(&rows).unwrap();
    assert!(reason.starts_with("      ⚠ conflict in"), "{reason:?}");
    assert!(!reason.contains(long.trim_end()), "{reason:?}");
    // Clipped to the pane, never drawn past its own edge.
    assert!(reason.chars().count() <= usize::from(NARROW), "{reason:?}");
    // PROJECTS header, the repo, the agent, and one reason line — the error
    // wrapped onto nothing.
    assert_eq!(drawn_rows(&rows), 4, "{rows:?}");
}

/// The reason line is not a `Selection`, so `j` steps from the failed agent
/// straight to the next one.
#[test]
fn navigation_steps_over_the_reason_line() {
    let mut app = app_with(&["agt-a", "agt-b"]);
    set_error(&mut app, "agt-a", Some("conflicts"));
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
/// put when a row gains or loses its reason line.
#[test]
fn selection_survives_a_row_gaining_and_losing_the_line() {
    let mut app = app_with(&["agt-a", "agt-b"]);
    app.selected = app
        .visible()
        .iter()
        .position(|item| matches!(item, Selection::Agent { agent: 1, .. }))
        .unwrap();
    let before = app.selected_item().map(|item| app.item_key(item));

    set_error(&mut app, "agt-a", Some("conflicts"));
    app.restore_selection(before.clone());
    assert_eq!(app.selected_item().map(|item| app.item_key(item)), before);
    assert!(reason_row(&rows(&app)).is_some());

    set_error(&mut app, "agt-a", None);
    app.restore_selection(before.clone());
    assert_eq!(app.selected_item().map(|item| app.item_key(item)), before);
}
