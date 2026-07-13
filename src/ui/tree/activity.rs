use ratatui::text::Span;

use crate::model::Status;

use super::THEME;

pub(super) fn shows_process(status: Status) -> bool {
    matches!(status, Status::Waiting | Status::Done | Status::Idle)
}

pub(super) fn activity_spans(
    command: Option<&str>,
    subagents_active: usize,
    gap: &str,
) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(2);
    if let Some(command) = command {
        spans.push(Span::styled(
            format!("{gap}⚙ {}", truncate_command(command)),
            THEME.proc_style(),
        ));
    }
    if subagents_active > 0 {
        spans.push(Span::styled(
            format!("{gap}✻{subagents_active}"),
            THEME.subagent_style(),
        ));
    }
    spans
}

fn truncate_command(command: &str) -> String {
    const MAX: usize = 16;
    let mut chars = command.chars();
    let prefix: String = chars.by_ref().take(MAX).collect();
    if chars.next().is_some() {
        format!("{}…", prefix.chars().take(MAX - 1).collect::<String>())
    } else {
        prefix
    }
}
