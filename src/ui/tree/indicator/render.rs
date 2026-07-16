use ratatui::text::Span;

use crate::{model::Status, ui::theme::DEFAULT as THEME};

use super::{Indicator, SupplementaryIndicators};
use crate::ui::tree::activity::activity_span;

pub(in crate::ui::tree) fn spans(
    indicator: Option<Indicator>,
    supplementary: SupplementaryIndicators,
    selected: bool,
    elapsed: std::time::Duration,
    gap: &str,
) -> Vec<Span<'static>> {
    let mut spans = match indicator {
        Some(Indicator::Status(status)) => vec![Span::styled(
            format!("{gap}{}", status.glyph()),
            if selected {
                THEME.selected_status_style(status)
            } else {
                THEME.status_style(status)
            },
        )],
        Some(Indicator::Running) => vec![Span::styled(
            format!("{gap}{}", crate::ui::spinner::frame(elapsed)),
            if selected {
                THEME.selected_status_style(Status::Running)
            } else {
                THEME.status_style(Status::Running)
            },
        )],
        Some(Indicator::Merging) => vec![Span::styled(
            format!("{gap}⇄ {}", crate::ui::spinner::frame(elapsed)),
            THEME.hint_style(),
        )],
        Some(Indicator::ShellActivity) => vec![Span::styled(
            format!("{gap}{}", crate::ui::spinner::term_frame(elapsed)),
            THEME.term_style(),
        )],
        Some(Indicator::SubagentActivity(active)) => vec![activity_span(active, gap)],
        Some(Indicator::DroprRefresh) => vec![Span::styled(
            format!("{gap}{}", crate::ui::spinner::frame(elapsed)),
            THEME.hint_style(),
        )],
        None => Vec::new(),
    };
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
