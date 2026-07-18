use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Borders, Paragraph},
};

use crate::model::Selection;

use super::{IndicatorState, label, select, select_supplementary};
use crate::ui::{App, theme::DEFAULT as THEME};

pub(super) fn draw(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let selected = matches!(app.selected_item(), Some(Selection::Overseer));
    let style = if selected {
        THEME.selection_style()
    } else {
        THEME.accent_style()
    };
    let (summary, _) = crate::ui::overseer::summary(app);
    let state = IndicatorState::with_status(Some(crate::ui::overseer::status()));
    let primary = select(state);
    let right =
        super::indicator::supplementary_spans(primary, select_supplementary(state), selected, "  ");
    let line = label::labeled_row(
        area.width.saturating_sub(2),
        if selected { "> " } else { "  " }.into(),
        primary,
        &format!(
            "{}  inbox:{}/{} actionable  [a]nswer [y]approve [/]select",
            summary.strip_prefix("OVERSEER / ").unwrap_or(&summary),
            app.overseer_inbox
                .iter()
                .filter(|item| item.target_session.is_some())
                .count(),
            app.overseer_inbox.len(),
        ),
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
