//! The tree's box-drawing connector: one ancestor guide column per level
//! above a row, then the row's own branch glyph fused with its expand
//! handle. Split out of `label` because the marker layering above and the
//! box-drawing layering here are two independent concerns that both happen
//! to build the same row prefix.

use ratatui::{style::Style, text::Span};

use super::AGENT_INDENT;

/// A row's own expand affordance, fused onto the end of its connector rather
/// than floating separately in the row (cargo-tree-tui's `└─▼ quote`, not a
/// connector followed by a detached arrow). A leaf still spends this column —
/// with a plain dash rather than a triangle — so its content starts at the
/// same place an expandable sibling's does: nothing about the row shifts
/// depending on whether it happens to have children.
///
/// Collapsed and expanded both point down — the direction the subtree
/// actually opens — rather than collapsed pointing right and expanded
/// pointing down, as `▸`/`▾` used to. Weight carries the state instead:
/// hollow `▽` for collapsed, solid `▾` for expanded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui::tree) enum TreeHandle {
    Leaf,
    Collapsed,
    Expanded,
}

impl TreeHandle {
    fn glyph(self) -> &'static str {
        match self {
            Self::Leaf => "─",
            Self::Collapsed => "▽",
            Self::Expanded => "▾",
        }
    }
}

/// One ancestor's guide column: a continuing vertical bar when that ancestor
/// still has a later sibling below — more rows are coming at that column — or
/// blank when it was the last child in its own group, so nothing will ever
/// draw there again. Two columns wide, matching the width of a row's own
/// connector cell below it.
fn guide_column(continues: bool) -> &'static str {
    if continues { "│ " } else { "  " }
}

/// A row's own branch glyph, fused with its expand handle: corner, dash,
/// handle. Three columns so a leaf and an expandable row spend the same
/// width here and the title column never drifts between them.
fn connector(is_last: bool, handle: TreeHandle) -> String {
    let arm = if is_last { "└" } else { "├" };
    format!("{arm}─{}", handle.glyph())
}

/// The connector prefix shared by every row that descends from an agent:
/// [`AGENT_INDENT`]'s (zero-width) nesting step so a depth-0 connector lands
/// under the repo row's own fold icon, one guide column per ancestor
/// (continuing `│` or blank, per [`guide_column`]), then this row's own
/// fused branch+handle glyph.
pub(super) fn tree_prefix(
    cursor: &str,
    ancestor_continues: &[bool],
    is_last: bool,
    handle: TreeHandle,
) -> String {
    let guides: String = ancestor_continues
        .iter()
        .copied()
        .map(guide_column)
        .collect();
    format!(
        "{cursor} {AGENT_INDENT}{guides}{}",
        connector(is_last, handle)
    )
}

/// The plain connector prefix for a row that carries no management marker of
/// its own — currently only child-worktree rows, which hang directly off an
/// agent rather than being agents themselves.
pub(in crate::ui::tree) fn leaf_row_prefix(
    cursor: &str,
    ancestor_continues: &[bool],
    is_last: bool,
    structure: Style,
) -> Span<'static> {
    Span::styled(
        format!(
            "{} ",
            tree_prefix(cursor, ancestor_continues, is_last, TreeHandle::Leaf)
        ),
        structure,
    )
}
