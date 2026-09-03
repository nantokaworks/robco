use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::ui::{App, Mode, layout, theme::DEFAULT as THEME};

pub(super) fn draw(frame: &mut Frame<'_>, app: &App, area: Rect, message: Option<&str>) {
    let footer = layout::footer(area);

    let ident = Paragraph::new(Line::from(vec![
        Span::styled("ROBCO", THEME.accent_bold_style()),
        Span::styled(format!(" {}", footer.version), THEME.hint_style()),
    ]));
    frame.render_widget(ident, footer.zones.ident);

    let hints = Paragraph::new(super::hints::hints_line(
        app,
        message,
        app.selected_item(),
        app.dropr_task_focus,
        matches!(app.mode, Mode::TaskBody { .. }),
    ))
    .alignment(Alignment::Center);
    frame.render_widget(hints, footer.zones.hints);
}
