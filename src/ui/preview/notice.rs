use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::{
    model::Selection,
    ui::{App, merge_dialog, theme::DEFAULT as THEME},
};

use super::TAB_BAR_ROWS;

/// Transient merge overlay: the merge running right now, or one that finished
/// cleanly. Failures are not drawn here — they own a preview tab of their own,
/// which is what keeps this overlay short-lived enough to be acceptable at all.
pub(in crate::ui) fn render_merge_notice(
    frame: &mut Frame<'_>,
    app: &App,
    selection: Option<Selection>,
    area: Rect,
) {
    let notice = merge_dialog::notice_lines(app, selection);
    if notice.is_empty() {
        return;
    }
    let inner_width = area.width.saturating_sub(2).max(1);
    let rows: u16 = notice
        .iter()
        .map(|line| {
            let w = line.width() as u16;
            (w / inner_width + u16::from(!w.is_multiple_of(inner_width))).max(1)
        })
        .fold(0u16, |acc, r| acc.saturating_add(r));
    // Start one row down: `area.y` is the preview block's top border, which is
    // where the tab bar is drawn. Covering it would hide which tab is active and
    // which tabs exist for exactly as long as the notice stands.
    let top = area.y.saturating_add(TAB_BAR_ROWS);
    let available = area.height.saturating_sub(TAB_BAR_ROWS);
    if available == 0 {
        return;
    }
    let height = rows.saturating_add(2).min(available);
    let popup = Rect {
        x: area.x,
        y: top,
        width: area.width,
        height,
    };
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" merge ")
        .borders(Borders::ALL)
        .border_style(THEME.accent_style());
    let para = Paragraph::new(notice)
        .block(block)
        .style(THEME.accent_style())
        .wrap(Wrap { trim: false });
    frame.render_widget(para, popup);
}
