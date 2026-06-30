use ratatui::layout::{Constraint, Direction, Layout, Rect};

const TREE_WIDTH_RATIO: f32 = 0.30;
const TREE_MIN_WIDTH: u16 = 24;
const TREE_MAX_WIDTH: u16 = 48;

pub(crate) struct RootLayout {
    pub(crate) header: Rect,
    pub(crate) body: Rect,
    pub(crate) footer: Rect,
}

pub(crate) struct PaneLayout {
    pub(crate) tree: Rect,
    pub(crate) preview: Rect,
}

pub(crate) fn root(area: Rect) -> RootLayout {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    RootLayout {
        header: chunks[0],
        body: chunks[1],
        footer: chunks[2],
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
