use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
};

use crate::model::Selection;

use super::App;

const TREE_WIDTH_RATIO: f32 = 0.30;
const TREE_MIN_WIDTH: u16 = 24;
const TREE_MAX_WIDTH: u16 = 48;

pub(crate) struct RootLayout {
    pub(crate) body: Rect,
    pub(crate) footer: Rect,
}

pub(crate) struct PaneLayout {
    pub(crate) tree: Rect,
    pub(crate) preview: Rect,
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

pub(crate) fn panes(body: Rect) -> PaneLayout {
    let tree_width = tree_width(body.width);
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(tree_width), Constraint::Min(0)])
        .split(body);

    PaneLayout {
        tree: chunks[0],
        preview: chunks[1],
    }
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
    let tree = panes(container).tree;

    let width = width.min(container.width);
    let height = height.min(container.height);

    // The tree block reserves its top row for the "PROJECTS" title.
    let anchor_row = tree.y + 1 + selected_row_offset(app, visible);

    let x = tree.x.min(container.right().saturating_sub(width));
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
    for (idx, item) in visible.iter().enumerate() {
        if idx == app.selected {
            break;
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
