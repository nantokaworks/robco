use ratatui::text::Span;

use crate::{model::Status, ui::theme::DEFAULT as THEME};

use super::{Indicator, SupplementaryIndicators};
use crate::ui::tree::label;

pub(in crate::ui::tree) fn primary_span(
    indicator: Option<Indicator>,
    selected: bool,
    elapsed: std::time::Duration,
    width: usize,
) -> Span<'static> {
    let (glyph, style) = match indicator {
        Some(Indicator::Status(status)) => (
            status.glyph().to_string(),
            if selected {
                THEME.selected_status_style(status)
            } else {
                THEME.status_style(status)
            },
        ),
        Some(Indicator::Running) => (
            crate::ui::spinner::frame(elapsed).to_string(),
            if selected {
                THEME.selected_status_style(Status::Running)
            } else {
                THEME.status_style(Status::Running)
            },
        ),
        Some(Indicator::Merging) => ("⇄".to_string(), THEME.hint_style()),
        Some(Indicator::ShellActivity) => (
            crate::ui::spinner::term_frame(elapsed).to_string(),
            THEME.term_style(),
        ),
        Some(Indicator::SubagentActivity(_)) => ("✻".to_string(), THEME.subagent_style()),
        Some(Indicator::DroprRefresh) => (
            crate::ui::spinner::frame(elapsed).to_string(),
            THEME.hint_style(),
        ),
        None => (String::new(), THEME.accent_style()),
    };
    Span::styled(label::pad_to_width(&glyph, width), style)
}

pub(in crate::ui::tree) fn supplementary_spans(
    indicator: Option<Indicator>,
    supplementary: SupplementaryIndicators,
    selected: bool,
    gap: &str,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    if let Some(Indicator::SubagentActivity(active)) = indicator {
        spans.push(Span::styled(
            format!("{gap}{active}"),
            THEME.subagent_style(),
        ));
    }
    if supplementary.worktree_missing {
        let prefix = if spans.is_empty() { gap } else { " " };
        spans.push(Span::styled(
            format!("{prefix}⌦"),
            THEME.worktree_missing_style(selected),
        ));
    }
    if supplementary.merge_failed {
        let prefix = if spans.is_empty() { gap } else { " " };
        spans.push(Span::styled(
            format!("{prefix}merge-failed"),
            THEME.merge_failed_style(selected),
        ));
    }
    spans
}
