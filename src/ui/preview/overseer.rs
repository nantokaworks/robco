use ratatui::text::{Line, Span, Text};

use crate::{overseer, ui::theme::DEFAULT as THEME};

use super::super::App;

pub(super) fn control_preview(app: &App) -> (String, Text<'static>) {
    let session = overseer::control_session_name(&app.config.tmux_session_prefix);
    // The background capture worker mirrors the control session; an empty/absent
    // session yields no cached text, which reads as "not started".
    let text = app.cached_tmux(&session).unwrap_or_else(not_started);
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
