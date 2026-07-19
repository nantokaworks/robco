use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
};

use crate::model::Selection;

use super::App;

const TREE_WIDTH_RATIO: f32 = 0.30;
const TREE_MIN_WIDTH: u16 = 24;
const TREE_MAX_WIDTH: u16 = 48;
pub(in crate::ui) const OVERSEER_FRAME_MIN_HEIGHT: u16 = 3;
pub(in crate::ui) const OVERSEER_FRAME_MAX_HEIGHT: u16 = 16;

pub(crate) struct RootLayout {
    pub(crate) body: Rect,
    pub(crate) footer: Rect,
}

pub(crate) struct PaneLayout {
    pub(crate) overseer: Rect,
    pub(crate) tree: Rect,
    pub(crate) preview: Rect,
}

pub(crate) struct TreeStackLayout {
    pub(crate) overseer: Rect,
    pub(crate) projects: Rect,
}

/// Split of the bottom status row: the `ROBCO v<x.y.z>` identity segment is
/// pinned bottom-left (retro CRT prompt-home position), key hints fill the rest.
pub(crate) struct FooterZones {
    pub(crate) ident: Rect,
    pub(crate) hints: Rect,
}

pub(crate) fn footer_zones(footer: Rect, ident_width: u16) -> FooterZones {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(ident_width), Constraint::Min(0)])
        .split(footer);

    FooterZones {
        ident: chunks[0],
        hints: chunks[1],
    }
}

pub(crate) fn root(area: Rect) -> RootLayout {
    // CRT-bezel breathing space: a constant 1-row top margin and 1-column
    // left/right margins. The bottom edge stays untouched so the status row
    // keeps sitting on the last terminal row (a Layout::margin(1) would lift
    // it off). Saturating arithmetic keeps tiny terminals from underflowing.
    let area = Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(1),
    };

    // Two rows only: the body fills everything, and a single status row at the
    // bottom carries the ROBCO brand (left) plus key hints. The old top banner
    // row is gone — the brand now lives in the bottom-left status line.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    RootLayout {
        body: chunks[0],
        footer: chunks[1],
    }
}

pub(crate) fn panes(body: Rect, overseer_frame_height: u16) -> PaneLayout {
    let tree_width = tree_width(body.width);
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(tree_width), Constraint::Min(0)])
        .split(body);

    let tree = tree_stack(chunks[0], overseer_frame_height);
    PaneLayout {
        overseer: tree.overseer,
        tree: tree.projects,
        preview: chunks[1],
    }
}

pub(crate) fn tree_stack(tree: Rect, overseer_frame_height: u16) -> TreeStackLayout {
    if overseer_frame_height == 0 {
        return TreeStackLayout {
            overseer: Rect::default(),
            projects: tree,
        };
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(overseer_frame_height),
            Constraint::Min(0),
        ])
        .split(tree);

    TreeStackLayout {
        overseer: chunks[0],
        projects: chunks[1],
    }
}

pub(in crate::ui) fn overseer_frame_height(content_rows: usize) -> u16 {
    u16::try_from(content_rows)
        .unwrap_or(u16::MAX)
        .saturating_add(2)
        .clamp(OVERSEER_FRAME_MIN_HEIGHT, OVERSEER_FRAME_MAX_HEIGHT)
}

fn tree_width(body_width: u16) -> u16 {
    if body_width <= TREE_MIN_WIDTH {
        return body_width.saturating_sub(1);
    }

    ((body_width as f32 * TREE_WIDTH_RATIO) as u16)
        .clamp(TREE_MIN_WIDTH, TREE_MAX_WIDTH)
        .min(body_width.saturating_sub(1))
}

pub(in crate::ui) fn centered_area(frame: &Frame<'_>, width: u16, height: u16) -> Rect {
    let container = root(frame.area()).body;

    let width = width.min(container.width);
    let height = height.min(container.height);
    let x = container.x + container.width.saturating_sub(width) / 2;
    let y = container.y + container.height.saturating_sub(height) / 2;

    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Place the dialog just below the selected tree row, clamped inside the
/// content pane. Falls back to above the row when there is no room below.
pub(in crate::ui) fn popup_area(
    frame: &Frame<'_>,
    app: &App,
    visible: &[Selection],
    width: u16,
    height: u16,
) -> Rect {
    let container = root(frame.area()).body;
    let panes = panes(container, app.overseer_frame_height());

    let width = width.min(container.width);
    let height = height.min(container.height);

    // Each bordered frame reserves its top row for its title/border.
    let anchor_row = if matches!(
        app.selected_item(),
        Some(Selection::Overseer | Selection::OverseerCategory(_))
    ) {
        let content = super::tree::overseer_frame::content_lines(app);
        let inner_height = panes.overseer.height.saturating_sub(2);
        let row = content
            .selected_row
            .saturating_sub(content.scroll_offset(inner_height));
        panes.overseer.y.saturating_add(1).saturating_add(row)
    } else {
        panes.tree.y + 1 + selected_row_offset(app, visible)
    };

    let x = panes.tree.x.min(container.right().saturating_sub(width));
    let below = anchor_row.saturating_add(1);
    let y = if below + height <= container.bottom() {
        below
    } else {
        anchor_row.saturating_sub(height)
    };
    let y = y
        .max(container.y)
        .min(container.bottom().saturating_sub(height));

    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Number of rendered rows above the selected item, accounting for the extra
/// "(no agents)" line drawn under an expanded empty repo.
fn selected_row_offset(app: &App, visible: &[Selection]) -> u16 {
    let mut offset = 0u16;
    let selected = app.selected_item();
    for item in visible {
        if Some(*item) == selected {
            break;
        }
        if matches!(item, Selection::Overseer | Selection::OverseerCategory(_)) {
            continue;
        }
        offset += 1;
        if let Selection::Repo(repo_idx) = item {
            let expanded = app.expanded.get(*repo_idx).copied().unwrap_or(true);
            if expanded && app.registry.repos[*repo_idx].agents.is_empty() {
                offset += 1;
            }
        }
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_insets_top_and_sides_and_keeps_footer_on_last_row() {
        let layout = root(Rect::new(0, 0, 80, 24));

        assert_eq!(layout.body, Rect::new(1, 1, 78, 22));
        // Footer sits on the last terminal row, inset one column on each side.
        assert_eq!(layout.footer, Rect::new(1, 23, 78, 1));
    }

    #[test]
    fn root_inset_is_relative_to_a_non_origin_area() {
        let layout = root(Rect::new(5, 3, 40, 10));

        assert_eq!(layout.body, Rect::new(6, 4, 38, 8));
        assert_eq!(layout.footer, Rect::new(6, 12, 38, 1));
    }

    #[test]
    fn root_does_not_underflow_on_tiny_terminals() {
        for (width, height) in [(0, 0), (1, 1), (2, 1), (1, 2), (2, 2), (3, 3)] {
            let layout = root(Rect::new(0, 0, width, height));

            let expected_width = width.saturating_sub(2);
            assert_eq!(layout.body.width, expected_width);
            assert_eq!(layout.footer.width, expected_width);
            // The inset area never extends past the original bottom edge
            // (the y+1 shift itself is the only row on a height-0 area).
            assert!(layout.footer.bottom() <= height.max(1));
        }
    }

    #[test]
    fn tree_stack_reserves_overseer_frame_above_projects() {
        let layout = tree_stack(Rect::new(2, 4, 32, 20), 8);

        assert_eq!(layout.overseer, Rect::new(2, 4, 32, 8));
        assert_eq!(layout.projects, Rect::new(2, 12, 32, 12));
    }

    #[test]
    fn tree_stack_gives_projects_full_height_without_overseer() {
        let tree = Rect::new(2, 4, 32, 20);
        let layout = tree_stack(tree, 0);

        assert_eq!(layout.overseer, Rect::default());
        assert_eq!(layout.projects, tree);
    }

    #[test]
    fn overseer_frame_height_clamps_content_plus_border() {
        assert_eq!(overseer_frame_height(0), OVERSEER_FRAME_MIN_HEIGHT);
        assert_eq!(overseer_frame_height(5), 7);
        assert_eq!(overseer_frame_height(usize::MAX), OVERSEER_FRAME_MAX_HEIGHT);
    }
}
