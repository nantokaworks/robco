use std::time::Duration;

use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::indicator::{self, Indicator};

const START_PAUSE: Duration = Duration::from_millis(1_000);
const STEP: Duration = Duration::from_millis(300);
const END_PAUSE: Duration = Duration::from_millis(1_000);

/// Marks an agent row as an Overseer worker under automatic dispatch. It sits at
/// the row head so a column of agents can be scanned at a glance; Manual workers
/// and unmanaged worktrees render blank there (the OVERSEER pane still reports
/// the difference).
const OVERSEER_AUTO_MARKER: &str = "◆";

/// One nesting step, applied to every agent row so its title starts right of
/// the repo name above it and the row reads as a child of that repo. The marker
/// cell sits left of this, at the row head, and does not buy the containment
/// back — it occupies the cell that would otherwise have read as indentation.
///
/// Rows that must track the agent title column carry this too: the child-worktree
/// row and the empty-repo filler in the parent module.
pub(super) const AGENT_INDENT: &str = "  ";

/// The prefix of an agent row: cursor, Overseer marker cell, the nesting step
/// under the repo, the identity-tree indent, then the expand arrow for a row
/// that has child worktrees.
///
/// The marker spends one of the three cells that already separated the cursor
/// from the indent, so the title column sits at the same offset whether or not
/// a row carries it.
pub(super) fn agent_row_prefix(
    cursor: &str,
    overseer_auto: bool,
    depth: usize,
    child_marker: Option<&str>,
) -> String {
    let marker = if overseer_auto {
        OVERSEER_AUTO_MARKER
    } else {
        " "
    };
    format!(
        "{cursor} {marker} {AGENT_INDENT}{}{}",
        "  ".repeat(depth),
        child_marker.unwrap_or("")
    )
}

pub(super) fn display(title: &str, available: usize, selected: bool, elapsed: Duration) -> String {
    if UnicodeWidthStr::width(title) <= available {
        title.to_string()
    } else if selected {
        marquee(title, available, elapsed)
    } else {
        truncate(title, available)
    }
}

pub(super) fn available_width<'a>(
    row_width: u16,
    prefix: &str,
    indicator_width: usize,
    right: impl IntoIterator<Item = &'a Span<'a>>,
) -> usize {
    let right_width = right
        .into_iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum::<usize>();
    usize::from(row_width)
        .saturating_sub(UnicodeWidthStr::width(prefix))
        .saturating_sub(indicator_width)
        .saturating_sub(right_width)
}

pub(super) fn fit_prefix(prefix: &str, row_width: u16) -> String {
    prefix_within(prefix, usize::from(row_width).saturating_sub(2)).to_string()
}

pub(super) fn trim_spans_to_width(spans: &mut Vec<Span<'static>>, max_width: usize) {
    let mut remaining = max_width;
    spans.retain_mut(|span| {
        if remaining == 0 {
            return false;
        }
        let content = prefix_within(span.content.as_ref(), remaining).to_string();
        let width = UnicodeWidthStr::width(content.as_str());
        let keep = !content.is_empty();
        span.content = content.into();
        remaining = remaining.saturating_sub(width);
        keep
    });
}

pub(super) fn pad_to_width(value: &str, width: usize) -> String {
    let value = prefix_within(value, width);
    let padding = width.saturating_sub(UnicodeWidthStr::width(value));
    format!("{value}{}", " ".repeat(padding))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn labeled_row(
    row_width: u16,
    prefix: String,
    primary: Option<Indicator>,
    title: &str,
    prefix_style: Style,
    title_style: Style,
    selected: bool,
    elapsed: Duration,
    mut right: Vec<Span<'static>>,
) -> Line<'static> {
    let prefix = fit_prefix(&prefix, row_width);
    let prefix_width = UnicodeWidthStr::width(prefix.as_str());
    let indicator_width = if primary.is_some() {
        usize::from(row_width).saturating_sub(prefix_width).min(2)
    } else {
        0
    };
    let width = available_width(row_width, &prefix, indicator_width, &right);
    let title = display(title, width, selected, elapsed);
    let primary = indicator::primary_span(primary, selected, elapsed, indicator_width);
    let used = prefix_width + indicator_width + UnicodeWidthStr::width(title.as_str());
    trim_spans_to_width(&mut right, usize::from(row_width).saturating_sub(used));
    let mut spans = vec![
        Span::styled(prefix, prefix_style),
        primary,
        Span::styled(title, title_style),
    ];
    spans.extend(right);
    Line::from(spans)
}

fn truncate(title: &str, available: usize) -> String {
    if UnicodeWidthStr::width(title) <= available {
        return title.to_string();
    }
    if available == 0 {
        return String::new();
    }

    let content_width = available - 1;
    let mut result = prefix_within(title, content_width).to_string();
    result.push('…');
    result
}

fn marquee(title: &str, available: usize, elapsed: Duration) -> String {
    if available == 0 {
        return String::new();
    }
    let offset = marquee_offset(UnicodeWidthStr::width(title), available, elapsed);
    let start = byte_at_or_after_width(title, offset);
    prefix_within(&title[start..], available).to_string()
}

fn marquee_offset(title_width: usize, available: usize, elapsed: Duration) -> usize {
    let max_offset = title_width.saturating_sub(available);
    if max_offset == 0 {
        return 0;
    }
    let travel = STEP * u32::try_from(max_offset).unwrap_or(u32::MAX);
    let cycle = START_PAUSE + travel + END_PAUSE;
    let position = elapsed.as_millis() % cycle.as_millis();
    let start_ms = START_PAUSE.as_millis();
    if position <= start_ms {
        0
    } else if position >= (START_PAUSE + travel).as_millis() {
        max_offset
    } else {
        usize::try_from((position - start_ms) / STEP.as_millis())
            .unwrap_or(max_offset)
            .min(max_offset)
    }
}

fn byte_at_or_after_width(value: &str, target: usize) -> usize {
    let mut width = 0;
    for (index, character) in value.char_indices() {
        if width >= target {
            return index;
        }
        width += UnicodeWidthChar::width(character).unwrap_or(0);
    }
    value.len()
}

fn prefix_within(value: &str, max_width: usize) -> &str {
    let mut width = 0;
    let mut end = 0;
    for (index, character) in value.char_indices() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if width + character_width > max_width {
            break;
        }
        width += character_width;
        end = index + character.len_utf8();
    }
    &value[..end]
}

#[cfg(test)]
mod tests;
