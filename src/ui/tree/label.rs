use std::time::Duration;

use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::config::ProjectIcon;
use crate::model::ManagementMode;
use crate::overseer::is_overseer_child;

use super::indicator::{self, Indicator};

mod connector;
use connector::tree_prefix;
pub(super) use connector::{TreeHandle, leaf_row_prefix};

mod marquee;
use marquee::display;

/// What the Overseer does with an agent row, drawn as one glyph left of the
/// row's title.
///
/// The base (non-Nerdfont) rendering keeps the round glyph family the tree has
/// always used for this: the tree already spends triangles and box-drawing on
/// structure — `▸`/`▾` expand handles, `└` child connectors — so a triangular
/// state marker landing next to a handle reads as a second, smaller handle
/// rather than as state (this is why `#362`'s Nerdfont pair below stays away
/// from arrow/triangle glyphs too — a Nerdfont "play" icon would reintroduce
/// exactly that collision with the `▸` expand handle). Filled means the
/// Overseer dispatches to the row on its own; hollow means it owns the row but
/// waits to be told; blank means the row is not the Overseer's to drive.
/// `#362` layers a Nerdfont-only pair on top (bolt/hand) for operators who
/// opted into `project_icon = "nerdfont"` — see [`Self::glyph`] — since a
/// stronger pictographic contrast is only safe to draw once a patched font is
/// known to be present.
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
    ///
    /// `icon` gates the Nerdfont pair the same way [`ProjectIcon::marker`]
    /// gates the folder glyphs: a Nerdfont codepoint must never be drawn
    /// unconditionally, since it renders as tofu without a patched font.
    /// `ProjectIcon::None` and `ProjectIcon::Emoji` both fall back to the
    /// round glyphs — reusing `project_icon` here means an emoji-icon
    /// operator, who has not necessarily opted into Nerdfont glyphs, still
    /// gets the safe default rather than a second unrelated pictograph
    /// family.
    fn glyph(self, icon: ProjectIcon) -> &'static str {
        match (self, icon) {
            (Self::Unmanaged, _) => " ",
            (Self::Auto, ProjectIcon::Nerdfont) => "\u{f0e7}", // nf-fa-bolt
            (Self::Manual, ProjectIcon::Nerdfont) => "\u{f256}", // nf-fa-hand_paper_o
            (Self::Auto, ProjectIcon::None | ProjectIcon::Emoji) => "●",
            (Self::Manual, ProjectIcon::None | ProjectIcon::Emoji) => "○",
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

    /// Blanks a Manual marker that only repeats its repo's own Manual state,
    /// so an agent row's marker cell is reserved for the cases where it
    /// actually says something the repo row does not already say. The
    /// visibility of the indicator is the point: a marker on every row reads
    /// as wallpaper.
    ///
    /// `#362`: Auto is exempt and never blanks here, even when it matches its
    /// repo. A repo in Auto mode is the common case, and blanking every Auto
    /// agent under it used to render that agent identically to an unmanaged
    /// worktree — the exact ambiguity this marker exists to prevent. Manual
    /// keeps the original wallpaper-avoidance behaviour, since a Manual
    /// agent blanking under a Manual repo never collides with Auto (Auto is
    /// now always drawn) and only collides with Unmanaged, which is the
    /// lower-signal state operators already read from the OVERSEER pane.
    pub(super) fn unless_matching(self, repo: Self) -> Self {
        if self == Self::Manual && repo == Self::Manual {
            Self::Unmanaged
        } else {
            self
        }
    }
}

/// The repo row's own management glyph, shown unconditionally (unlike an
/// agent row's, which blanks out via [`ManagementMarker::unless_matching`]
/// when it only repeats this).
pub(super) fn repo_management_glyph(
    management: ManagementMode,
    icon: ProjectIcon,
    style: Style,
) -> Span<'static> {
    Span::styled(ManagementMarker::of_repo(management).glyph(icon), style)
}

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

/// The prefix of an agent row: cursor, the nesting step under the repo, the
/// ancestor guide columns, this row's own connector fused with its expand
/// handle, then the management marker cell.
///
/// Returned as spans rather than one string because the prefix carries two
/// different kinds of information and they must not be drawn at the same
/// weight. `structure` covers the indentation, guides, and connector — where
/// the row sits; `marker` covers the management glyph — what the Overseer does
/// with it. Rendering both through one style is what let the old play glyph
/// blur into the expand arrow beside it.
///
/// The prefix reserves the marker cell either way — an unmanaged row renders it
/// blank — so neither the title column nor the connector's own column moves
/// whether or not a row carries a marker.
#[allow(clippy::too_many_arguments)]
pub(super) fn agent_row_prefix(
    cursor: &str,
    management: ManagementMarker,
    icon: ProjectIcon,
    ancestor_continues: &[bool],
    is_last: bool,
    handle: TreeHandle,
    structure: Style,
    marker: Style,
) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            format!(
                "{} ",
                tree_prefix(cursor, ancestor_continues, is_last, handle)
            ),
            structure,
        ),
        Span::styled(management.glyph(icon), marker),
        Span::styled(" ", structure),
    ]
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
mod marker_tests;
#[cfg(test)]
mod prefix_tests;
#[cfg(test)]
mod tests;
