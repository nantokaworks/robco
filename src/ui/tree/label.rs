use std::time::Duration;

use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::indicator::{self, Indicator};

const START_PAUSE: Duration = Duration::from_millis(1_000);
const STEP: Duration = Duration::from_millis(300);
const END_PAUSE: Duration = Duration::from_millis(1_000);

pub(super) fn display(title: &str, available: usize, selected: bool, elapsed: Duration) -> String {
    if UnicodeWidthStr::width(title) <= available {
        title.to_string()
    } else if selected {
        marquee(title, available, elapsed)
    } else {
        truncate(title, available)
    }
}

pub(super) fn available_width<'a>(
    row_width: u16,
    prefix: &str,
    right: impl IntoIterator<Item = &'a Span<'a>>,
) -> usize {
    let right_width = right
        .into_iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum::<usize>();
    usize::from(row_width)
        .saturating_sub(UnicodeWidthStr::width(prefix))
        .saturating_sub(2)
        .saturating_sub(right_width)
}

pub(super) fn fit_prefix(prefix: &str, row_width: u16) -> String {
    prefix_within(prefix, usize::from(row_width).saturating_sub(2)).to_string()
}

pub(super) fn trim_spans_to_width(spans: &mut Vec<Span<'static>>, max_width: usize) {
    let mut remaining = max_width;
    spans.retain_mut(|span| {
        if remaining == 0 {
            return false;
        }
        let content = prefix_within(span.content.as_ref(), remaining).to_string();
        let width = UnicodeWidthStr::width(content.as_str());
        let keep = !content.is_empty();
        span.content = content.into();
        remaining = remaining.saturating_sub(width);
        keep
    });
}

pub(super) fn pad_to_width(value: &str, width: usize) -> String {
    let value = prefix_within(value, width);
    let padding = width.saturating_sub(UnicodeWidthStr::width(value));
    format!("{value}{}", " ".repeat(padding))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn labeled_row(
    row_width: u16,
    prefix: String,
    primary: Option<Indicator>,
    title: &str,
    prefix_style: Style,
    title_style: Style,
    selected: bool,
    elapsed: Duration,
    mut right: Vec<Span<'static>>,
) -> Line<'static> {
    let prefix = fit_prefix(&prefix, row_width);
    let width = available_width(row_width, &prefix, &right);
    let title = display(title, width, selected, elapsed);
    let prefix_width = UnicodeWidthStr::width(prefix.as_str());
    let indicator_width = usize::from(row_width).saturating_sub(prefix_width).min(2);
    let primary = indicator::primary_span(primary, selected, elapsed, indicator_width);
    let used = prefix_width + indicator_width + UnicodeWidthStr::width(title.as_str());
    trim_spans_to_width(&mut right, usize::from(row_width).saturating_sub(used));
    let mut spans = vec![
        Span::styled(prefix, prefix_style),
        primary,
        Span::styled(title, title_style),
    ];
    spans.extend(right);
    Line::from(spans)
}

fn truncate(title: &str, available: usize) -> String {
    if UnicodeWidthStr::width(title) <= available {
        return title.to_string();
    }
    if available == 0 {
        return String::new();
    }

    let content_width = available - 1;
    let mut result = prefix_within(title, content_width).to_string();
    result.push('…');
    result
}

fn marquee(title: &str, available: usize, elapsed: Duration) -> String {
    if available == 0 {
        return String::new();
    }
    let offset = marquee_offset(UnicodeWidthStr::width(title), available, elapsed);
    let start = byte_at_or_after_width(title, offset);
    prefix_within(&title[start..], available).to_string()
}

fn marquee_offset(title_width: usize, available: usize, elapsed: Duration) -> usize {
    let max_offset = title_width.saturating_sub(available);
    if max_offset == 0 {
        return 0;
    }
    let travel = STEP * u32::try_from(max_offset).unwrap_or(u32::MAX);
    let cycle = START_PAUSE + travel + END_PAUSE;
    let position = elapsed.as_millis() % cycle.as_millis();
    let start_ms = START_PAUSE.as_millis();
    if position <= start_ms {
        0
    } else if position >= (START_PAUSE + travel).as_millis() {
        max_offset
    } else {
        usize::try_from((position - start_ms) / STEP.as_millis())
            .unwrap_or(max_offset)
            .min(max_offset)
    }
}

fn byte_at_or_after_width(value: &str, target: usize) -> usize {
    let mut width = 0;
    for (index, character) in value.char_indices() {
        if width >= target {
            return index;
        }
        width += UnicodeWidthChar::width(character).unwrap_or(0);
    }
    value.len()
}

fn prefix_within(value: &str, max_width: usize) -> &str {
    let mut width = 0;
    let mut end = 0;
    for (index, character) in value.char_indices() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if width + character_width > max_width {
            break;
        }
        width += character_width;
        end = index + character.len_utf8();
    }
    &value[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_ascii_with_ellipsis() {
        let result = display("abcdefgh", 5, false, Duration::ZERO);
        assert_eq!(result, "abcd…");
        assert!(UnicodeWidthStr::width(result.as_str()) <= 5);
    }

    #[test]
    fn truncates_cjk_on_character_boundaries() {
        let result = display("日本語です", 6, false, Duration::ZERO);
        assert_eq!(result, "日本…");
        assert!(UnicodeWidthStr::width(result.as_str()) <= 6);
    }

    #[test]
    fn leaves_fitting_title_unchanged() {
        assert_eq!(display("agent", 5, false, Duration::ZERO), "agent");
    }

    #[test]
    fn handles_zero_and_one_column_widths() {
        for available in 0..=1 {
            let result = display("agent", available, false, Duration::ZERO);
            assert!(UnicodeWidthStr::width(result.as_str()) <= available);
        }
    }

    #[test]
    fn handles_empty_exact_fit_and_too_wide_character() {
        assert_eq!(display("", 1, false, Duration::ZERO), "");
        assert_eq!(display("agent", 5, false, Duration::ZERO), "agent");
        assert_eq!(display("日", 1, false, Duration::ZERO), "…");
    }

    #[test]
    fn marquee_is_elapsed_driven_bounded_and_loops() {
        let title = "abcdefgh";
        let width = 4;
        assert_eq!(marquee_offset(8, width, Duration::ZERO), 0);
        assert_eq!(marquee_offset(8, width, Duration::from_millis(900)), 0);

        let advanced = display(title, width, true, Duration::from_millis(1_600));
        assert_ne!(advanced, "abcd");
        assert!(UnicodeWidthStr::width(advanced.as_str()) <= width);
        assert!(title.contains(&advanced));

        let cycle = START_PAUSE + STEP * 4 + END_PAUSE;
        assert_eq!(display(title, width, true, cycle), "abcd");
    }

    #[test]
    fn marquee_never_splits_wide_characters() {
        let result = display("日本語です", 4, true, Duration::from_millis(1_600));
        assert!(UnicodeWidthStr::width(result.as_str()) <= 4);
        assert!("日本語です".contains(&result));
    }

    #[test]
    fn marquee_offset_honors_phase_boundaries() {
        let max_offset = 4;
        let travel = STEP * max_offset;
        let cycle = START_PAUSE + travel + END_PAUSE;
        assert_eq!(marquee_offset(8, 4, START_PAUSE), 0);
        assert_eq!(marquee_offset(8, 4, START_PAUSE + STEP), 1);
        assert_eq!(
            marquee_offset(8, 4, START_PAUSE + travel),
            max_offset as usize
        );
        assert_eq!(marquee_offset(8, 4, cycle), 0);
    }

    #[test]
    fn fits_prefix_indicator_title_and_overflowing_right_spans() {
        let pane_width = 12;
        let row = labeled_row(
            pane_width,
            ">   ".to_string(),
            None,
            "agent",
            Style::default(),
            Style::default(),
            false,
            Duration::ZERO,
            vec![Span::raw("  supplementary")],
        );
        let row_width = row
            .spans
            .iter()
            .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
            .sum::<usize>();
        assert!(row_width <= usize::from(pane_width));
    }
}
