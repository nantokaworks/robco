use ratatui::text::Span;

use super::THEME;

pub(super) fn activity_span(subagents_active: usize, gap: &str) -> Span<'static> {
    Span::styled(format!("{gap}✻{subagents_active}"), THEME.subagent_style())
}
