//! The agent row's prefix: the management marker's three states, the layering
//! that keeps the marker from reading as one more expand handle, and the
//! box-drawing connector each row draws under its ancestors.

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};
use unicode_width::UnicodeWidthStr;

use super::*;

/// The two prefix styles a text assertion does not care about, so a prefix
/// reads as one string rather than as three spans.
fn prefix(
    cursor: &str,
    management: ManagementMarker,
    ancestor_continues: &[bool],
    is_last: bool,
    handle: TreeHandle,
) -> String {
    agent_row_prefix(
        cursor,
        management,
        ancestor_continues,
        is_last,
        handle,
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
    assert_eq!(
        prefix(">", ManagementMarker::Auto, &[], true, TreeHandle::Leaf),
        ">     └── ● "
    );
    assert_eq!(
        prefix(">", ManagementMarker::Manual, &[], true, TreeHandle::Leaf),
        ">     └── ○ "
    );
    assert_eq!(
        prefix(
            ">",
            ManagementMarker::Unmanaged,
            &[],
            true,
            TreeHandle::Leaf
        ),
        ">     └──   "
    );
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
    let column = |depth: usize| {
        let ancestors = vec![true; depth];
        prefix(
            " ",
            ManagementMarker::Auto,
            &ancestors,
            true,
            TreeHandle::Leaf,
        )
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
/// the same column on every state, at every depth, with or without children.
#[test]
fn the_marker_does_not_move_the_title_column() {
    for (ancestors, handle) in [
        (vec![], TreeHandle::Leaf),
        (vec![true], TreeHandle::Leaf),
        (vec![true, false], TreeHandle::Expanded),
    ] {
        let width = |management| {
            UnicodeWidthStr::width(prefix(" ", management, &ancestors, true, handle).as_str())
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

/// A leaf spends the same handle column an expandable row does — with a plain
/// dash instead of a triangle — so neither the marker nor the title drifts
/// depending on whether a row happens to have children.
#[test]
fn a_leaf_spends_the_same_handle_column_as_an_expandable_row() {
    let leaf = prefix(" ", ManagementMarker::Auto, &[], true, TreeHandle::Leaf);
    let expanded = prefix(" ", ManagementMarker::Auto, &[], true, TreeHandle::Expanded);
    let collapsed = prefix(
        " ",
        ManagementMarker::Auto,
        &[],
        true,
        TreeHandle::Collapsed,
    );
    assert_eq!(
        UnicodeWidthStr::width(leaf.as_str()),
        UnicodeWidthStr::width(expanded.as_str())
    );
    assert_eq!(
        UnicodeWidthStr::width(leaf.as_str()),
        UnicodeWidthStr::width(collapsed.as_str())
    );
}

/// The branch glyph is `└` for a last child and `├` when a later sibling
/// follows, and the expand handle fuses directly onto it — no floating arrow
/// elsewhere in the row.
#[test]
fn the_connector_reflects_last_sibling_and_fuses_the_handle() {
    assert_eq!(
        prefix(
            " ",
            ManagementMarker::Unmanaged,
            &[],
            true,
            TreeHandle::Expanded
        ),
        "      └─▾   "
    );
    assert_eq!(
        prefix(
            " ",
            ManagementMarker::Unmanaged,
            &[],
            false,
            TreeHandle::Collapsed
        ),
        "      ├─▸   "
    );
}

/// A nested row's guide over an ancestor's column continues (`│`) when that
/// ancestor still has a later sibling below, and goes blank otherwise — the
/// last-sibling case must hold at every depth, not just the row's own.
#[test]
fn ancestor_guides_continue_or_blank_independently_of_the_rows_own_branch() {
    assert_eq!(
        prefix(
            " ",
            ManagementMarker::Unmanaged,
            &[true],
            true,
            TreeHandle::Leaf
        ),
        "      │ └──   "
    );
    assert_eq!(
        prefix(
            " ",
            ManagementMarker::Unmanaged,
            &[false],
            true,
            TreeHandle::Leaf
        ),
        "        └──   "
    );
    assert_eq!(
        prefix(
            " ",
            ManagementMarker::Unmanaged,
            &[true, false],
            false,
            TreeHandle::Leaf
        ),
        "      │   ├──   "
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
        &[true],
        false,
        TreeHandle::Collapsed,
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
        "the expand handle belongs to the structure layer: {spans:?}"
    );
}
