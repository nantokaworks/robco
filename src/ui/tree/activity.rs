use ratatui::text::Span;

use super::THEME;

pub(super) fn activity_spans(subagents_active: usize, gap: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(1);
    if subagents_active > 0 {
        spans.push(Span::styled(
            format!("{gap}✻{subagents_active}"),
            THEME.subagent_style(),
        ));
    }
    spans
}
