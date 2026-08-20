//! The agent row's prefix layout: the box-drawing connector each row draws
//! under its ancestors, and where a child-worktree row's connector lines up
//! with its owning agent's.

use ratatui::style::Style;
use unicode_width::UnicodeWidthStr;

use super::*;

/// A prefix reads as one string rather than as separate spans, since these
/// tests care about structure/placement, not styling.
fn prefix(cursor: &str, ancestor_continues: &[bool], is_last: bool, handle: TreeHandle) -> String {
    agent_row_prefix(
        cursor,
        ancestor_continues,
        is_last,
        handle,
        Style::default(),
    )
    .iter()
    .map(|span| span.content.as_ref())
    .collect()
}

/// Child-worktree rows hang off an agent through `leaf_row_prefix`, which
/// wraps the same `tree_prefix` an agent row's own connector uses. `#327`'s
/// fix pulls an agent connector under the repo row's fold icon by changing
/// `AGENT_INDENT`; this confirms `leaf_row_prefix` carries that change too,
/// rather than assuming it does because both call the same helper.
#[test]
fn a_child_worktree_row_shares_its_owning_agents_connector_column() {
    let ancestors = [true];
    let is_last = false;
    let agent = agent_row_prefix(
        ">",
        &ancestors,
        is_last,
        TreeHandle::Expanded,
        Style::default(),
    );
    let agent_prefix: String = agent.iter().map(|span| span.content.as_ref()).collect();
    let child = leaf_row_prefix(">", &ancestors, is_last, Style::default());

    let connector_column = |content: &str| {
        content
            .find('├')
            .expect("prefix carries its own branch connector")
    };
    assert_eq!(
        connector_column(&agent_prefix),
        connector_column(child.content.as_ref()),
        "child-worktree connector does not line up with its owning agent's: agent={agent_prefix:?} child={:?}",
        child.content
    );
}

/// A leaf spends the same handle column an expandable row does — with a plain
/// dash instead of a triangle — so the title does not drift depending on
/// whether a row happens to have children.
#[test]
fn a_leaf_spends_the_same_handle_column_as_an_expandable_row() {
    let leaf = prefix(" ", &[], true, TreeHandle::Leaf);
    let expanded = prefix(" ", &[], true, TreeHandle::Expanded);
    let collapsed = prefix(" ", &[], true, TreeHandle::Collapsed);
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
    assert_eq!(prefix(" ", &[], true, TreeHandle::Expanded), "  └─▾ ");
    assert_eq!(prefix(" ", &[], false, TreeHandle::Collapsed), "  ├─▸ ");
}

/// A nested row's guide over an ancestor's column continues (`│`) when that
/// ancestor still has a later sibling below, and goes blank otherwise — the
/// last-sibling case must hold at every depth, not just the row's own.
#[test]
fn ancestor_guides_continue_or_blank_independently_of_the_rows_own_branch() {
    assert_eq!(prefix(" ", &[true], true, TreeHandle::Leaf), "  │ └── ");
    assert_eq!(prefix(" ", &[false], true, TreeHandle::Leaf), "    └── ");
    assert_eq!(
        prefix(" ", &[true, false], false, TreeHandle::Leaf),
        "  │   ├── "
    );
}
