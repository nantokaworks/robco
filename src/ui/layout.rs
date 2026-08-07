use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
};

const TREE_WIDTH_RATIO: f32 = 0.30;
const TREE_MIN_WIDTH: u16 = 24;
const TREE_MAX_WIDTH: u16 = 48;
pub(in crate::ui) const OVERSEER_FRAME_MIN_HEIGHT: u16 = 2;
pub(in crate::ui) const OVERSEER_FRAME_MAX_HEIGHT: u16 = 15;

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

pub(crate) struct FooterLayout {
    pub(crate) version: String,
    pub(crate) zones: FooterZones,
    pub(crate) caret: (u16, u16),
}

pub(crate) fn footer(footer: Rect) -> FooterLayout {
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    let brand_width = ("ROBCO ".len() + version.chars().count()) as u16;
    let zones = footer_zones(footer, brand_width.saturating_add(2));
    let caret_bounds = if zones.ident.width > 0 {
        zones.ident
    } else {
        footer
    };
    let caret_min = caret_bounds.x;
    let caret_max = caret_bounds
        .x
        .saturating_add(caret_bounds.width.saturating_sub(1));
    let caret_x = zones
        .ident
        .x
        .saturating_add(brand_width)
        .saturating_add(1)
        .clamp(caret_min, caret_max);

    FooterLayout {
        version,
        caret: (caret_x, caret_bounds.y),
        zones,
    }
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
    // The extra row is the trailing spacer between OVERSEER and PROJECTS.
    u16::try_from(content_rows)
        .unwrap_or(u16::MAX)
        .saturating_add(1)
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

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
