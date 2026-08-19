use ratatui::text::Span;

use crate::{model::Status, ui::theme::DEFAULT as THEME};

use super::{Indicator, SupplementaryIndicators};
use crate::ui::tree::label;

pub(in crate::ui) fn primary_span(
    indicator: Option<Indicator>,
    selected: bool,
    elapsed: std::time::Duration,
    width: usize,
) -> Span<'static> {
    let (glyph, base_style) = match indicator {
        Some(Indicator::Status(status)) => (status.glyph().to_string(), THEME.status_style(status)),
        Some(Indicator::Running) => (
            crate::ui::spinner::frame(elapsed).to_string(),
            THEME.status_style(Status::Running),
        ),
        Some(Indicator::Merging) => ("⇄".to_string(), THEME.hint_style()),
        Some(Indicator::McpActivity) => (
            crate::ui::spinner::mcp_frame(elapsed).to_string(),
            THEME.mcp_style(),
        ),
        Some(Indicator::ShellActivity) => (
            crate::ui::spinner::term_frame(elapsed).to_string(),
            THEME.term_style(),
        ),
        Some(Indicator::SubagentActivity(_)) => ("✻".to_string(), THEME.subagent_style()),
        Some(Indicator::DroprRefresh) => (
            crate::ui::spinner::frame(elapsed).to_string(),
            THEME.hint_style(),
        ),
        Some(Indicator::MergeLifecycle(lifecycle)) => (
            lifecycle.glyph().to_string(),
            THEME.merge_lifecycle_style(lifecycle),
        ),
        None => (String::new(), THEME.accent_style()),
    };
    // On a selected row every indicator glyph — including the empty `None`
    // padding — joins the reversed selection bar so there is no hollow gap.
    let style = if selected {
        THEME.selected_indicator_style()
    } else {
        base_style
    };
    Span::styled(label::pad_to_width(&glyph, width), style)
}

pub(in crate::ui) fn supplementary_spans(
    indicator: Option<Indicator>,
    supplementary: SupplementaryIndicators,
    selected: bool,
    gap: &str,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    // When the row is selected, every supplementary span inverts with the row
    // (selection background) instead of leaving a hollow gap.
    let sel = THEME.selected_indicator_style();
    if let Some(Indicator::SubagentActivity(active)) = indicator {
        let style = if selected {
            sel
        } else {
            THEME.subagent_style()
        };
        spans.push(Span::styled(format!("{gap}{active}"), style));
    }
    if supplementary.worktree_missing {
        let prefix = if spans.is_empty() { gap } else { " " };
        let style = if selected {
            sel
        } else {
            THEME.worktree_missing_style(false)
        };
        spans.push(Span::styled(format!("{prefix}⌦"), style));
    }
    if supplementary.merge_failed {
        let prefix = if spans.is_empty() { gap } else { " " };
        let style = if selected {
            sel
        } else {
            THEME.merge_failed_style(false)
        };
        spans.push(Span::styled(format!("{prefix}merge-failed"), style));
    }
    if supplementary.needs_decision {
        let prefix = if spans.is_empty() { gap } else { " " };
        let style = if selected {
            sel
        } else {
            THEME.needs_decision_style(false)
        };
        spans.push(Span::styled(format!("{prefix}blocked"), style));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MergeLifecycle;

    #[test]
    fn selected_empty_indicator_carries_selection_background() {
        let span = primary_span(None, true, std::time::Duration::ZERO, 2);
        assert_eq!(span.style.bg, Some(THEME.selection_bg));
        assert_eq!(span.style.fg, Some(THEME.selection_fg));
    }

    #[test]
    fn unselected_empty_indicator_has_no_selection_background() {
        let span = primary_span(None, false, std::time::Duration::ZERO, 2);
        assert_eq!(span.style.bg, None);
    }

    #[test]
    fn selected_supplementary_spans_carry_selection_background() {
        let spans = supplementary_spans(
            None,
            SupplementaryIndicators {
                worktree_missing: true,
                merge_failed: true,
                needs_decision: true,
            },
            true,
            " ",
        );
        for span in &spans {
            assert_eq!(span.style.bg, Some(THEME.selection_bg));
            assert_eq!(span.style.fg, Some(THEME.selection_fg));
        }
    }

    #[test]
    fn merge_lifecycle_renders_its_own_glyph_not_the_done_glyph() {
        let span = primary_span(
            Some(Indicator::MergeLifecycle(MergeLifecycle::ChecksFailing)),
            false,
            std::time::Duration::ZERO,
            2,
        );
        assert_eq!(span.content.trim(), MergeLifecycle::ChecksFailing.glyph());
        assert_ne!(span.content.trim(), Status::Done.glyph());
    }

    #[test]
    fn needs_decision_renders_a_blocked_badge() {
        let spans = supplementary_spans(
            None,
            SupplementaryIndicators {
                worktree_missing: false,
                merge_failed: false,
                needs_decision: true,
            },
            false,
            " ",
        );
        let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(text, " blocked");
    }
}
