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
/// `▸`/`▾` stay as they were — the "points the wrong way" complaint is
/// addressed by [`AGENT_INDENT`] instead: once the connector hangs directly
/// under the repo row's fold icon, the guide column itself leads the eye
/// straight down into the subtree, so the handle glyph does not have to
/// carry that direction on its own.
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
            Self::Collapsed => "▸",
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

/// The guide-only prefix for a line that continues the row above it rather
/// than hanging below it: an agent's reason line (dropr:518). It draws no
/// branch glyph of its own, because it is not a row — it is the tail of one.
///
/// The columns it does draw are the ones a *child* of that agent would draw:
/// every ancestor guide, then one more column for the agent itself, which
/// continues when the agent still has a later sibling. That is the same
/// `deeper.push(!is_last)` step `crate::model::agent_tree::agent_rows` takes for real
/// children, so the vertical run down to the next sibling is never broken,
/// and the line sits flush against whatever child row follows it.
pub(in crate::ui::tree) fn continuation_prefix(
    ancestor_continues: &[bool],
    is_last: bool,
) -> String {
    let guides: String = ancestor_continues
        .iter()
        .copied()
        .map(guide_column)
        .collect();
    // Two leading columns for the cursor the continuation never has, matching
    // `tree_prefix`'s own `"{cursor} "`.
    format!("  {AGENT_INDENT}{guides}{}", guide_column(!is_last))
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
