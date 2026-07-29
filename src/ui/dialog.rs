use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::model::Selection;

use super::{App, Mode, help, layout, theme::DEFAULT as THEME};

mod caret;
mod content;
#[cfg(test)]
mod tests;

use caret::caret_position;
use content::DialogContent;

pub fn draw(frame: &mut Frame<'_>, app: &App, visible: &[Selection]) -> Option<(u16, u16)> {
    let body = layout::root(frame.area()).body;
    let DialogContent {
        title,
        lines,
        caret,
    } = content::content(app, body)?;

    let width = (lines
        .iter()
        .map(Line::width)
        .max()
        .unwrap_or(0)
        .max(title.len()) as u16
        + 4)
    .min(body.width);
    let height = (lines.len() as u16 + 2).min(body.height);
    let area = if matches!(&app.mode, Mode::Help { .. }) {
        layout::centered_area(frame, width, height)
    } else {
        layout::popup_area(frame, app, visible, width, height)
    };

    let (title, scroll) = match app.mode {
        Mode::Help { scroll } => (
            help::scroll_title(scroll, frame.area().height, app.locale).unwrap_or(title),
            help::clamp_scroll(scroll, frame.area().height),
        ),
        // A dialog taller than the body it must fit into scrolls just far enough
        // to keep the edited row on screen, wherever in the text the caret is.
        _ => (title, caret.map_or(0, |(row, _)| scroll_for(row, height))),
    };
    let cursor = caret.map(|(row, column)| caret_position(area, column, row, scroll));
    let block = Block::default()
        .title(title)
        .title_style(Style::default().add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(THEME.dialog_border_style());
    let dialog = Paragraph::new(lines)
        .scroll((scroll, 0))
        .block(block)
        .style(THEME.accent_style());
    frame.render_widget(Block::default().style(THEME.backdrop_style()), body);

    // `Clear` only resets cells *inside* the popup rect, so a full-width (CJK)
    // glyph in the dimmed background that straddles the popup's left/right border
    // would leave a stray half-cell that corrupts the border. Wiping the whole
    // row-band removes any such glyph; rows above and below stay dimmed.
    let band = Rect {
        x: body.x,
        y: area.y,
        width: body.width,
        height: area.height,
    };
    frame.render_widget(Clear, band);
    // Painting the backdrop under the popup would leave its DIM modifier
    // (`set_style` only adds modifiers), rendering the dialog content dim.
    let right_x = area.x + area.width;
    for side in [
        Rect {
            x: band.x,
            y: band.y,
            width: area.x.saturating_sub(band.x),
            height: band.height,
        },
        Rect {
            x: right_x,
            y: band.y,
            width: (band.x + band.width).saturating_sub(right_x),
            height: band.height,
        },
    ] {
        frame.render_widget(Block::default().style(THEME.backdrop_style()), side);
    }
    frame.render_widget(dialog, area);
    cursor
}

/// Smallest scroll offset that leaves content row `row` inside a popup of
/// `height` rows (two of which are the border).
fn scroll_for(row: usize, height: u16) -> u16 {
    let visible_rows = height.saturating_sub(2);
    u16::try_from(row)
        .unwrap_or(u16::MAX)
        .saturating_add(1)
        .saturating_sub(visible_rows)
}
