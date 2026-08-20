//! The scrollable dialog for reading one dropr task's full body in place
//! (dropr:501): opened by `Enter` on a task-list row (`Mode::TaskBody`),
//! drawn over the task list, which stays exactly as it was underneath — its
//! own render call, cursor, and scroll are never touched while this is open.
//!
//! Shares its chrome (centered popup, `Clear`, dimmed backdrop) with the
//! generic dialog in `super::draw`, but is dispatched separately because its
//! content is per-task and dynamic — unlike every mode `super::content`
//! handles, which builds from static, translated strings.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::model::Selection;

use super::super::{App, layout, summary::dropr_task_body, theme::DEFAULT as THEME};

pub(super) fn draw(
    frame: &mut Frame<'_>,
    app: &App,
    task: usize,
    scroll: u16,
) -> Option<(u16, u16)> {
    let body = layout::root(frame.area()).body;
    let Some(Selection::Repo(repo)) = app.selected_item() else {
        return None;
    };
    let (title, text) = dropr_task_body(&app.registry.repos[repo], task, app.locale)?;
    let lines = text.lines;

    let width = (lines
        .iter()
        .map(Line::width)
        .max()
        .unwrap_or(0)
        .max(title.len()) as u16
        + 4)
    .min(body.width);
    let height = (lines.len() as u16 + 2).min(body.height);
    let area = layout::centered_area(frame, width, height);
    // Clamped here, not in `ui::input`'s scroll keys: the content length
    // varies per task, so only the draw call — which already has the
    // rendered line count — can bound it without re-rendering the body twice.
    let max_scroll = (lines.len() as u16).saturating_sub(height.saturating_sub(2));
    let scroll = scroll.min(max_scroll);

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
    // Same reasoning as `super::draw`'s own `band`/`side` painting: `Clear`
    // only resets cells inside the popup rect, so a full-width glyph in the
    // dimmed background straddling the popup's border would corrupt it —
    // wipe the whole row-band, then re-dim only the columns outside `area`.
    let band = Rect {
        x: body.x,
        y: area.y,
        width: body.width,
        height: area.height,
    };
    frame.render_widget(Clear, band);
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
    // No text caret in a read-only dialog — the footer caret stays parked
    // where `event_loop::run_loop` last put it.
    None
}

#[cfg(test)]
#[path = "task_body_tests.rs"]
mod tests;
