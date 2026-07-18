use ratatui::{
    layout::Rect,
    text::{Line, Span, Text},
};

use crate::{overseer, tmux, ui::theme::DEFAULT as THEME};

use super::super::{App, scrollback};

pub(super) fn control_preview(app: &App, area: Rect, scroll: u16) -> (String, Text<'static>) {
    let session = overseer::control_session_name(&app.config.tmux_session_prefix);
    let text = if tmux::has_session(&session).unwrap_or(false) {
        scrollback::capture(&session, area, scroll).unwrap_or_else(not_started)
    } else {
        not_started()
    };
    ("OVERSEER / control".to_string(), text)
}

fn not_started() -> Text<'static> {
    vec![
        Line::from(Span::styled(
            "Control session not started.",
            THEME.muted_style(),
        )),
        Line::from(Span::styled(
            "Press i to send an instruction, or Enter to start and attach.",
            THEME.muted_style(),
        )),
    ]
    .into()
}
