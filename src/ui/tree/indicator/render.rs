use ratatui::text::Span;

use crate::{
    model::{MergeLifecycle, Status},
    ui::theme::DEFAULT as THEME,
};

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
        // Animated, not the static `⇄` it used to be (dropr:545): a robco
        // thread really is running `git` and `gh` for as long as this shows,
        // and a still glyph read as "nothing is happening".
        Some(Indicator::Merging) => (
            crate::ui::spinner::robco_frame(elapsed).to_string(),
            THEME.hint_style(),
        ),
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
    // dropr:545. Static text, never a spinner: robco has accepted the merge
    // but nothing is running this instant — the daemon may be a whole poll
    // interval away. Suppressed when the primary glyph is already
    // `ApprovedWaiting`, which says the same thing; the badge exists for the
    // rows that glyph cannot reach. Drawn after the subagent count so that
    // count stays next to the `✻` it belongs to.
    let already_glyphed = matches!(
        indicator,
        Some(Indicator::MergeLifecycle(MergeLifecycle::ApprovedWaiting))
    );
    if supplementary.merge_queued && !already_glyphed {
        let prefix = if spans.is_empty() { gap } else { " " };
        let style = if selected {
            sel
        } else {
            THEME.merge_queued_style(false)
        };
        spans.push(Span::styled(format!("{prefix}merge-queued"), style));
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
    if supplementary.worker_finished {
        let prefix = if spans.is_empty() { gap } else { " " };
        let style = if selected {
            sel
        } else {
            THEME.worker_finished_style(false)
        };
        spans.push(Span::styled(format!("{prefix}worker-done"), style));
    }
    spans
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

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
                merge_queued: false,
                worktree_missing: true,
                merge_failed: true,
                needs_decision: true,
                worker_finished: true,
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
                merge_queued: false,
                worktree_missing: false,
                merge_failed: false,
                needs_decision: true,
                worker_finished: false,
            },
            false,
            " ",
        );
        let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(text, " blocked");
    }

    /// dropr:545: robco accepted the merge, so the row says so — statically,
    /// because nothing is running yet.
    #[test]
    fn merge_queued_renders_a_static_badge() {
        let spans = supplementary_spans(
            None,
            SupplementaryIndicators {
                merge_queued: true,
                worktree_missing: false,
                merge_failed: false,
                needs_decision: false,
                worker_finished: false,
            },
            false,
            " ",
        );
        let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(text, " merge-queued");
    }

    /// The primary glyph already says "approved, waiting". The badge must not
    /// repeat it.
    #[test]
    fn merge_queued_is_suppressed_when_the_glyph_already_says_it() {
        let spans = supplementary_spans(
            Some(Indicator::MergeLifecycle(MergeLifecycle::ApprovedWaiting)),
            SupplementaryIndicators {
                merge_queued: true,
                worktree_missing: false,
                merge_failed: false,
                needs_decision: false,
                worker_finished: false,
            },
            false,
            " ",
        );
        assert!(spans.is_empty());
    }

    /// Any other merge-lifecycle glyph means something different, so the
    /// badge still has something of its own to say.
    #[test]
    fn merge_queued_still_shows_next_to_a_different_lifecycle_glyph() {
        let spans = supplementary_spans(
            Some(Indicator::MergeLifecycle(MergeLifecycle::ChecksRunning)),
            SupplementaryIndicators {
                merge_queued: true,
                worktree_missing: false,
                merge_failed: false,
                needs_decision: false,
                worker_finished: false,
            },
            false,
            " ",
        );
        let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(text, " merge-queued");
    }

    /// A merge robco is running right now must read as motion, not as the
    /// still `⇄` it used to be (dropr:545).
    #[test]
    fn a_running_merge_animates() {
        let first = primary_span(Some(Indicator::Merging), false, Duration::ZERO, 2);
        let second = primary_span(
            Some(Indicator::Merging),
            false,
            crate::ui::spinner::FRAME_INTERVAL,
            2,
        );
        assert_ne!(first.content, second.content);
    }

    #[test]
    fn worker_finished_renders_a_worker_done_badge() {
        let spans = supplementary_spans(
            None,
            SupplementaryIndicators {
                merge_queued: false,
                worktree_missing: false,
                merge_failed: false,
                needs_decision: false,
                worker_finished: true,
            },
            false,
            " ",
        );
        let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(text, " worker-done");
    }
}
