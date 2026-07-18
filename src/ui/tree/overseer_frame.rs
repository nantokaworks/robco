use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::model::Selection;

use super::{label, select, select_supplementary, IndicatorState};
use crate::ui::{theme::DEFAULT as THEME, App};

pub(super) fn draw(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let selected = matches!(app.selected_item(), Some(Selection::Overseer));
    let style = if selected {
        THEME.selection_style()
    } else {
        THEME.accent_style()
    };
    let (summary, _) = crate::ui::overseer::summary();
    let state = IndicatorState::with_status(Some(crate::ui::overseer::status()));
    let primary = select(state);
    let right =
        super::indicator::supplementary_spans(primary, select_supplementary(state), selected, "  ");
    let line = label::labeled_row(
        area.width.saturating_sub(2),
        if selected { "> " } else { "  " }.into(),
        primary,
        summary.strip_prefix("OVERSEER / ").unwrap_or(&summary),
        style,
        style,
        selected,
        app.started.elapsed(),
        right,
    );
    let block = Block::default()
        .title("OVERSEER")
        .borders(Borders::ALL)
        .border_style(style)
        .title_style(Style::default().add_modifier(Modifier::BOLD));

    frame.render_widget(Paragraph::new(line).block(block).style(style), area);
}
