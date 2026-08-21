use std::time::Duration;

use ratatui::{
    style::Style,
    text::{Line, Span},
};

use crate::ui::text_width::{display_width, prefix_within};

use super::indicator::{self, Indicator};

mod connector;
use connector::tree_prefix;
pub(super) use connector::{TreeHandle, continuation_prefix, leaf_row_prefix};

mod marquee;
use marquee::display;

/// The nesting step between an agent row's cursor and its own connector.
/// Empty: a depth-0 connector must land in the same column as the repo row's
/// fold icon (`repo_row::build`, `ProjectIcon::marker`), and that column —
/// cursor width plus one separating space — never moves with the icon's own
/// display width. `ProjectIcon::Emoji` renders its glyphs at two cells instead
/// of one (`config.rs::ProjectIcon::marker`), which pushes everything *after*
/// the icon to the right, but never the icon's own starting column. So the
/// same zero offset is correct under every `ProjectIcon` setting — checked for
/// both `None` and `Emoji` in `tree::tests`, not merely assumed.
///
/// Rows that must track the agent title column carry this too: the child-worktree
/// row and the empty-repo filler in the parent module.
pub(super) const AGENT_INDENT: &str = "";

/// Columns [`labeled_row`] reserves for the primary indicator, between the
/// prefix and the title. Named so a line that must land in the same column
/// without going through `labeled_row` — an agent's reason line
/// (`super::reason_line`) — cannot drift away from it.
pub(super) const INDICATOR_WIDTH: usize = 2;

/// The prefix of an agent row: cursor, the nesting step under the repo, the
/// ancestor guide columns, then this row's own connector fused with its
/// expand handle.
pub(super) fn agent_row_prefix(
    cursor: &str,
    ancestor_continues: &[bool],
    is_last: bool,
    handle: TreeHandle,
    structure: Style,
) -> Vec<Span<'static>> {
    vec![Span::styled(
        format!(
            "{} ",
            tree_prefix(cursor, ancestor_continues, is_last, handle)
        ),
        structure,
    )]
}

fn available_width<'a>(
    row_width: u16,
    prefix_width: usize,
    indicator_width: usize,
    right: impl IntoIterator<Item = &'a Span<'a>>,
) -> usize {
    let right_width = spans_width(right);
    usize::from(row_width)
        .saturating_sub(prefix_width)
        .saturating_sub(indicator_width)
        .saturating_sub(right_width)
}

fn spans_width<'a>(spans: impl IntoIterator<Item = &'a Span<'a>>) -> usize {
    spans
        .into_iter()
        .map(|span| display_width(span.content.as_ref()))
        .sum()
}

fn fit_prefix(prefix: &mut Vec<Span<'static>>, row_width: u16) {
    trim_spans_to_width(prefix, usize::from(row_width).saturating_sub(2));
}

pub(super) fn trim_spans_to_width(spans: &mut Vec<Span<'static>>, max_width: usize) {
    let mut remaining = max_width;
    spans.retain_mut(|span| {
        if remaining == 0 {
            return false;
        }
        let content = prefix_within(span.content.as_ref(), remaining).to_string();
        let width = display_width(content.as_str());
        let keep = !content.is_empty();
        span.content = content.into();
        remaining = remaining.saturating_sub(width);
        keep
    });
}

pub(super) fn pad_to_width(value: &str, width: usize) -> String {
    let value = prefix_within(value, width);
    let padding = width.saturating_sub(display_width(value));
    format!("{value}{}", " ".repeat(padding))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn labeled_row(
    row_width: u16,
    mut prefix: Vec<Span<'static>>,
    primary: Option<Indicator>,
    title: &str,
    title_style: Style,
    selected: bool,
    elapsed: Duration,
    mut right: Vec<Span<'static>>,
) -> Line<'static> {
    fit_prefix(&mut prefix, row_width);
    let prefix_width = spans_width(&prefix);
    let indicator_width = if primary.is_some() {
        usize::from(row_width)
            .saturating_sub(prefix_width)
            .min(INDICATOR_WIDTH)
    } else {
        0
    };
    let width = available_width(row_width, prefix_width, indicator_width, &right);
    let title = display(title, width, selected, elapsed);
    let primary = indicator::primary_span(primary, selected, elapsed, indicator_width);
    let used = prefix_width + indicator_width + display_width(title.as_str());
    trim_spans_to_width(&mut right, usize::from(row_width).saturating_sub(used));
    let mut spans = prefix;
    spans.push(primary);
    spans.push(Span::styled(title, title_style));
    spans.extend(right);
    Line::from(spans)
}

#[cfg(test)]
mod prefix_tests;
#[cfg(test)]
mod tests;
