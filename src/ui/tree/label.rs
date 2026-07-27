use std::time::Duration;

use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::model::ManagementMode;
use crate::overseer::is_overseer_child;

use super::indicator::{self, Indicator};

const START_PAUSE: Duration = Duration::from_millis(1_000);
const STEP: Duration = Duration::from_millis(300);
const END_PAUSE: Duration = Duration::from_millis(1_000);

/// What the Overseer does with an agent row, drawn as one round glyph left of
/// the row's title.
///
/// Round is the point. The tree already spends triangles and box-drawing on
/// structure — `▸`/`▾` expand handles, `└` child connectors — so a triangular
/// state marker landing next to a handle reads as a second, smaller handle
/// rather than as state. One glyph family per layer: round means management,
/// angular means structure.
///
/// Filled means the Overseer dispatches to the row on its own; hollow means it
/// owns the row but waits to be told; blank means the row is not the Overseer's
/// to drive. All three are rendered, because `g` cycles a worktree through
/// unmanaged -> Auto -> Manual and a marker that only showed Auto left two of
/// those three steps looking identical.
///
/// The marker sits right of the row's own indentation, so it travels with the
/// tree hierarchy and reads as an attribute of the indented agent rather than
/// as a sibling of the repo above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ManagementMarker {
    Auto,
    Manual,
    Unmanaged,
}

impl ManagementMarker {
    /// Overseer ownership lives in `parent_agent_id`; `management` only means
    /// anything once the Overseer owns the row. A worktree adopted from
    /// `worktree_root` is persisted as `Manual` while nobody owns it, so
    /// reading the stored mode alone would draw an unowned row as Manual. This
    /// is the same pairing `ui::input::management::cycle_step` reads.
    pub(super) fn of(parent_agent_id: Option<&str>, management: ManagementMode) -> Self {
        if !is_overseer_child(parent_agent_id) {
            return Self::Unmanaged;
        }
        match management {
            ManagementMode::Auto => Self::Auto,
            ManagementMode::Manual => Self::Manual,
        }
    }

    /// Single-column whichever state it is, so the prefix reserves the cell
    /// either way and neither the title column nor the expand handle's column
    /// moves with the marker.
    fn glyph(self) -> &'static str {
        match self {
            Self::Auto => "●",
            Self::Manual => "○",
            Self::Unmanaged => " ",
        }
    }

    /// The repo row's own marker: unlike an agent row, a repo has no
    /// `parent_agent_id` ownership question, so its two states map directly
    /// onto `RepoNode::management`.
    pub(super) fn of_repo(management: ManagementMode) -> Self {
        match management {
            ManagementMode::Auto => Self::Auto,
            ManagementMode::Manual => Self::Manual,
        }
    }

    /// Blanks a marker that only repeats its repo's own state, so an agent
    /// row's marker cell is reserved for the cases where it actually says
    /// something the repo row does not already say. The visibility of the
    /// indicator is the point: a marker on every row reads as wallpaper.
    pub(super) fn unless_matching(self, repo: Self) -> Self {
        if self == repo { Self::Unmanaged } else { self }
    }
}

/// The repo row's own management glyph, shown unconditionally (unlike an
/// agent row's, which blanks out via [`ManagementMarker::unless_matching`]
/// when it only repeats this).
pub(super) fn repo_management_glyph(management: ManagementMode, style: Style) -> Span<'static> {
    Span::styled(ManagementMarker::of_repo(management).glyph(), style)
}

/// One nesting step, applied to every agent row so its title starts right of
/// the repo name above it and the row reads as a child of that repo. The marker
/// cell sits right of this and of the identity-tree indent, so it is indented
/// along with the row instead of occupying a fixed column pinned to the row's
/// left edge.
///
/// Four columns wide, not two: the repo row now carries its own management
/// marker cell (glyph + separating space) ahead of its name, so the nesting
/// step has to clear that in addition to the plain two-column indent it always
/// covered, or a depth-0 agent row's own marker would land under the repo
/// row's title instead of right of it.
///
/// Rows that must track the agent title column carry this too: the child-worktree
/// row and the empty-repo filler in the parent module.
pub(super) const AGENT_INDENT: &str = "    ";

/// The prefix of an agent row: cursor, the nesting step under the repo, the
/// identity-tree indent, the management marker cell, then the expand arrow for
/// a row that has child worktrees.
///
/// Returned as spans rather than one string because the prefix carries two
/// different kinds of information and they must not be drawn at the same
/// weight. `structure` covers the indentation and the expand arrow — where the
/// row sits; `marker` covers the management glyph — what the Overseer does with
/// it. Rendering both through one style is what let the old play glyph blur
/// into the expand arrow beside it.
///
/// The prefix reserves the marker cell either way — an unmanaged row renders it
/// blank — so neither the title column nor the expand arrow's own column moves
/// whether or not a row carries a marker.
pub(super) fn agent_row_prefix(
    cursor: &str,
    management: ManagementMarker,
    depth: usize,
    child_marker: Option<&str>,
    structure: Style,
    marker: Style,
) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            format!("{cursor} {AGENT_INDENT}{}", "  ".repeat(depth)),
            structure,
        ),
        Span::styled(management.glyph(), marker),
        Span::styled(format!(" {}", child_marker.unwrap_or("")), structure),
    ]
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
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
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
        usize::from(row_width).saturating_sub(prefix_width).min(2)
    } else {
        0
    };
    let width = available_width(row_width, prefix_width, indicator_width, &right);
    let title = display(title, width, selected, elapsed);
    let primary = indicator::primary_span(primary, selected, elapsed, indicator_width);
    let used = prefix_width + indicator_width + UnicodeWidthStr::width(title.as_str());
    trim_spans_to_width(&mut right, usize::from(row_width).saturating_sub(used));
    let mut spans = prefix;
    spans.push(primary);
    spans.push(Span::styled(title, title_style));
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
mod prefix_tests;
#[cfg(test)]
mod tests;
