//! The reason line under an agent row that has something wrong with it, or
//! that the operator has no other way to see why it isn't moving
//! (dropr:518, widened by dropr:524 and dropr:529).
//!
//! A row that robco knows to be in a bad state already says *that* it is —
//! the row carries a badge, or the ledger phase behind it does. It never said
//! *why*, because the text only ever reached the Info pane, which is exactly
//! the trip the operator is trying to avoid while scanning the tree for what
//! went wrong. This draws that text right under the row.
//!
//! Three sources earn a line, and a row gets at most one:
//!
//! 1. [`AgentNode::merge_error`] — a merge the operator ran from this screen
//!    that failed (dropr:518).
//! 2. A ledger entry parked in a terminal phase the operator did not ask for
//!    — `failed` or `escalated` — carrying the Overseer's own reason for
//!    stopping (dropr:524, [`crate::ui::overseer::OverseerSnapshot::terminal_reason`]).
//! 3. A ledger entry still live and open, but held on a reason nothing will
//!    clear without a person or the worker acting — `checks_not_green`
//!    foremost among them (dropr:529,
//!    [`crate::ui::overseer::OverseerSnapshot::held_reason`]). Unlike the
//!    first two, this entry has not stopped: the line clears on its own the
//!    next time the ledger reloads with no hold recorded, exactly the way the
//!    row's own merge-lifecycle badge already does.
//!
//! The other states a row reduces to a glyph keep their glyph.
//! `worktree_missing` is a `bool` with no reason text to show. A blocked
//! worker's `needs_decision` badge says a person is still needed, which is a
//! different question from why the entry stopped; its row is `escalated` by
//! definition, so case 2 already gives it its reason.
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

/// The reason line for one agent, or `None` when the agent has nothing to
/// explain. Callers push it right after the agent's own row.
///
/// `stopped` is the Overseer's reason for a terminal ledger phase, already
/// gated by the caller the same way the row's own ledger-sourced badges are.
/// It is the second source tried, because `merge_error` is the newer of the
/// two by construction: it is set by a merge the operator ran in this
/// session, and pressing `m` again clears it, at which point this line falls
/// back to the ledger's reason. The Info pane still holds both in full.
///
/// `held` is the third and last source tried: the entry's live merge-hold
/// reason (dropr:529), already gated the same way. It comes last because it
/// is the weakest signal of the three — the entry has not stopped, and
/// `merge_error` or a terminal phase both mean something the operator asked
/// for or the daemon gave up on, which outrank a hold that may still clear on
/// its own next pass.
pub(super) fn build(
    agent: &AgentNode,
    stopped: Option<&str>,
    held: Option<&str>,
    row: &AgentRow,
    projects_width: u16,
) -> Option<Line<'static>> {
    let (reason, glyph, style) = if let Some(reason) = first_line(agent.merge_error.as_deref()) {
        (reason, FAILURE_GLYPH, THEME.merge_failed_style(false))
    } else if let Some(reason) = first_line(stopped) {
        (reason, FAILURE_GLYPH, THEME.merge_failed_style(false))
    } else {
        let reason = first_line(held)?;
        // The same glyph the row's own `MergeLifecycle::OnHold` badge already
        // uses, so the two read as one vocabulary rather than a second
        // warning symbol.
        (
            reason,
            crate::model::MergeLifecycle::OnHold.glyph(),
            THEME.merge_hold_style(false),
        )
    };

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
        // The glyph lands in the agent's own indicator column and the reason
        // starts in its title column, so the line reads as the row's own
        // tail. `merge_error` and `stopped` share one glyph and colour: both
        // mean the entry actually stopped, and giving each its own mark would
        // add to the row indicator vocabulary this change deliberately leaves
        // alone. `held` gets its own, because it must not read as the same
        // thing (see the module doc).
        Span::styled(label::pad_to_width(glyph, label::INDICATOR_WIDTH), style),
        Span::styled(reason.to_string(), style),
    ];
    label::trim_spans_to_width(&mut spans, usize::from(projects_width));
    // A pane too narrow to hold even the prefix would leave an empty line
    // behind, which says less than drawing nothing at all.
    if spans.is_empty() {
        return None;
    }
    Some(Line::from(spans))
}

/// Glyph for a row that actually stopped: `merge_error` or a terminal phase.
const FAILURE_GLYPH: &str = "⚠";

/// The first line of a reason, trimmed, or `None` when it has nothing to say.
///
/// First line only. A `merge_error` can be a whole `gh`/`git` dump — the Info
/// pane already prints every line of it — but a tree row has to keep a fixed
/// height, so the tree says what happened in one line and the pane stays the
/// place the rest lives.
fn first_line(reason: Option<&str>) -> Option<&str> {
    let reason = reason?;
    let first = reason.lines().next().unwrap_or(reason).trim();
    (!first.is_empty()).then_some(first)
}

#[cfg(test)]
#[path = "reason_line_hold_tests.rs"]
mod hold_tests;
#[cfg(test)]
#[path = "reason_line_phase_tests.rs"]
mod phase_tests;
#[cfg(test)]
#[path = "reason_line_test_support.rs"]
mod test_support;
#[cfg(test)]
#[path = "reason_line_tests.rs"]
mod tests;
