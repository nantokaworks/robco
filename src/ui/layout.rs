use ratatui::layout::{Constraint, Direction, Layout, Rect};

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
