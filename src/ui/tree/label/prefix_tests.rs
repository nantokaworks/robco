//! The agent row's prefix: the management marker's three states, and the
//! layering that keeps the marker from reading as one more expand handle.

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};
use unicode_width::UnicodeWidthStr;

use super::*;

/// The two prefix styles a text assertion does not care about, so a prefix
/// reads as one string rather than as three spans.
fn prefix(cursor: &str, management: ManagementMarker, depth: usize, child: Option<&str>) -> String {
    agent_row_prefix(
        cursor,
        management,
        depth,
        child,
        Style::default(),
        Style::default(),
    )
    .iter()
    .map(|span| span.content.as_ref())
    .collect()
}

/// Auto, Manual, and unmanaged are three distinguishable renderings — the old
/// prefix drew Manual and unmanaged identically even though `g` treats them as
/// separate steps.
#[test]
fn the_three_management_states_render_three_distinct_markers() {
    assert_eq!(prefix(">", ManagementMarker::Auto, 0, None), ">   ● ");
    assert_eq!(prefix(">", ManagementMarker::Manual, 0, None), ">   ○ ");
    assert_eq!(prefix(">", ManagementMarker::Unmanaged, 0, None), ">     ");
}

/// Ownership and mode are read together: a worktree adopted from
/// `worktree_root` carries a stale `Manual` while nobody owns it.
#[test]
fn only_an_overseer_owned_row_reads_its_management_mode() {
    let overseer = Some(crate::overseer::OVERSEER_AGENT_ID);
    let of = ManagementMarker::of;
    assert_eq!(of(overseer, ManagementMode::Auto), ManagementMarker::Auto);
    assert_eq!(
        of(overseer, ManagementMode::Manual),
        ManagementMarker::Manual
    );
    for unowned in [None, Some("some-other-agent")] {
        for mode in [ManagementMode::Auto, ManagementMode::Manual] {
            assert_eq!(of(unowned, mode), ManagementMarker::Unmanaged);
        }
    }
}

/// The marker rides the indentation, so a deeper row carries it further right.
#[test]
fn the_marker_moves_right_with_the_row_depth() {
    let column = |depth| {
        prefix(" ", ManagementMarker::Auto, depth, None)
            .find(ManagementMarker::Auto.glyph())
            .expect("marked prefix carries the marker")
    };
    assert!(column(1) > column(0));
    assert!(column(2) > column(1));
}

/// Every state spends exactly one column, so the cell the prefix reserves is
/// the same width whichever state lands in it.
#[test]
fn every_management_marker_spends_exactly_one_column() {
    for management in ALL {
        assert_eq!(UnicodeWidthStr::width(management.glyph()), 1);
    }
}

/// The marker spends a cell the prefix already reserved, so the title starts at
/// the same column on every state, at every depth, with or without an arrow.
#[test]
fn the_marker_does_not_move_the_title_column() {
    for (depth, child_marker) in [(0, None), (1, None), (2, Some("▾ "))] {
        let width = |management| {
            UnicodeWidthStr::width(prefix(" ", management, depth, child_marker).as_str())
        };
        for management in ALL {
            assert_eq!(width(management), width(ManagementMarker::Auto));
        }
    }
}

const ALL: [ManagementMarker; 3] = [
    ManagementMarker::Auto,
    ManagementMarker::Manual,
    ManagementMarker::Unmanaged,
];

#[test]
fn the_prefix_indents_by_depth_and_appends_the_expand_arrow() {
    let marked = prefix(" ", ManagementMarker::Auto, 2, Some("▸ "));
    assert_eq!(marked, "        ● ▸ ");
    assert_eq!(
        prefix(" ", ManagementMarker::Unmanaged, 1, None),
        "        "
    );
}

/// The whole point of returning spans: the structural cells and the state
/// marker are separately styled, so the marker does not read as one more expand
/// arrow sitting beside the real one.
#[test]
fn the_structure_and_the_state_marker_are_drawn_in_different_styles() {
    let structure = Style::default().fg(Color::DarkGray);
    let marker = Style::default()
        .fg(Color::Green)
        .add_modifier(Modifier::BOLD);
    let spans: Vec<Span<'static>> = agent_row_prefix(
        ">",
        ManagementMarker::Auto,
        1,
        Some("▸ "),
        structure,
        marker,
    );

    let accented: Vec<_> = spans
        .iter()
        .filter(|span| span.style == marker)
        .map(|span| span.content.as_ref())
        .collect();
    assert_eq!(accented, ["●"]);
    assert!(
        spans
            .iter()
            .filter(|span| span.style == structure)
            .any(|span| span.content.contains('▸')),
        "the expand arrow belongs to the structure layer: {spans:?}"
    );
}
