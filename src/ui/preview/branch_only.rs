use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph, Wrap},
};

use crate::locale::t;
use crate::model::Selection;

use super::{PREVIEW_PADDING, tabs::preview_tabs_line};
use crate::ui::{App, PreviewPane, merge_dialog, theme::DEFAULT as THEME};

pub(super) fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    preview: (&App, PreviewPane, Option<Selection>),
    title: String,
    branch: &str,
    ai_label: &str,
) {
    let (app, active, selection) = preview;
    let text = vec![
        Line::from(Span::styled(
            t(app.locale, "Worktree has been removed."),
            THEME.muted_style(),
        )),
        Line::from(vec![
            Span::styled("branch: ", THEME.muted_style()),
            Span::raw(branch.to_string()),
        ]),
        Line::from(Span::styled(
            t(app.locale, "Press x to delete the branch."),
            THEME.muted_style(),
        )),
    ];
    let mut block = Block::default()
        .title_top(preview_tabs_line(
            active,
            &app.preview_panes(selection),
            ai_label,
        ))
        .title_top(Line::from(title).right_aligned())
        .borders(Borders::ALL)
        .padding(Padding::uniform(PREVIEW_PADDING));
    if let Some(title) = merge_dialog::preview_title(app, selection) {
        block = block.title_bottom(title);
    }
    let preview = Paragraph::new(text)
        .block(block)
        .style(THEME.muted_style())
        .wrap(Wrap { trim: false });
    frame.render_widget(preview, area);
    super::render_merge_notice(frame, app, selection, area);
}
