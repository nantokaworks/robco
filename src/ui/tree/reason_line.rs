//! The reason line under a failed agent row (dropr:518).
//!
//! An agent that failed to merge already says *that* it failed — the row
//! carries a `merge-failed` badge. It never said *why*, because the text in
//! `AgentNode::merge_error` only ever reached the Info pane, which is exactly
//! the trip the operator is trying to avoid while scanning the tree for what
//! went wrong. This draws that text right under the row.
//!
//! Only `merge_error` earns a line, and a row gets at most one. The other
//! states a row reduces to a glyph keep their glyph: `worktree_missing` is a
//! `bool` with no reason text to show, a merge lifecycle is an enum bucket
//! rather than free text, and a blocked worker's reason already has its own
//! row in the OVERSEER Inbox (`overseer::inbox_rows::row_reason`), so
//! repeating it here would put one sentence on screen twice.
//!
//! The line is deliberately not a [`crate::model::Selection`]: it is the tail
//! of the row above, not a row of its own, so `j`/`k` step straight over it.
//! Nothing has to enforce that — `App::visible()` is what the cursor moves
//! through, and `tree::draw` has always been free to push more lines than it
//! has selections (`repo_row`'s `(no agents)` filler, every `launch_row`).

use ratatui::text::{Line, Span};

use crate::model::{AgentNode, AgentRow};
use crate::ui::theme::DEFAULT as THEME;

use super::label;

/// Columns between the end of [`label::continuation_prefix`] and the
/// indicator column. A row's own connector spends three columns on its
/// branch glyph plus one separating space, where the continuation spends two
/// on a guide column — so two are left to make up.
const CONNECTOR_TAIL: usize = 2;

/// The reason line for one agent, or `None` when the agent has no failure to
/// explain. Callers push it right after the agent's own row.
pub(super) fn build(
    agent: &AgentNode,
    row: &AgentRow,
    projects_width: u16,
) -> Option<Line<'static>> {
    let error = agent.merge_error.as_deref()?;
    // First line only. `merge_error` can be a whole `gh`/`git` dump — the
    // Info pane already prints every line of it — but a tree row has to keep
    // a fixed height, so the tree says what happened in one line and the pane
    // stays the place the rest lives.
    let reason = error.lines().next().unwrap_or(error).trim();
    if reason.is_empty() {
        return None;
    }

    let mut spans = vec![
        Span::styled(
            format!(
                "{}{}",
                label::continuation_prefix(&row.ancestor_continues, row.is_last),
                " ".repeat(CONNECTOR_TAIL)
            ),
            // Never the selection style, even under a selected agent: the
            // selection bar belongs to the row, and this line is below it —
            // the same way a failed Discord channel's error line stays out of
            // its own row's styling.
            THEME.tree_structure_style(false),
        ),
        // `⚠` lands in the agent's own indicator column and the reason starts
        // in its title column, so the line reads as the row's own tail. Both
        // keep the colour of the `merge-failed` badge above them.
        Span::styled(
            label::pad_to_width("⚠", label::INDICATOR_WIDTH),
            THEME.merge_failed_style(false),
        ),
        Span::styled(reason.to_string(), THEME.merge_failed_style(false)),
    ];
    label::trim_spans_to_width(&mut spans, usize::from(projects_width));
    // A pane too narrow to hold even the prefix would leave an empty line
    // behind, which says less than drawing nothing at all.
    if spans.is_empty() {
        return None;
    }
    Some(Line::from(spans))
}

#[cfg(test)]
#[path = "reason_line_tests.rs"]
mod tests;
