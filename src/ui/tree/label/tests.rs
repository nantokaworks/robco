use std::time::Duration;

use ratatui::{
    style::{Modifier, Style},
    text::Span,
};
use unicode_width::UnicodeWidthStr;

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
        vec![Span::raw(">   ")],
        None,
        "agent",
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

#[test]
fn collapsed_indicator_gives_title_two_more_columns() {
    let row = |primary| {
        labeled_row(
            8,
            vec![Span::raw("> ")],
            primary,
            "abcdef",
            Style::default(),
            false,
            Duration::ZERO,
            vec![Span::raw(" R")],
        )
    };

    let without_indicator = row(None);
    let with_indicator = row(Some(Indicator::Merging));

    assert_eq!(without_indicator.spans[2].content.as_ref(), "abc…");
    assert_eq!(with_indicator.spans[2].content.as_ref(), "a…");
}

#[test]
fn collapses_indicator_column_when_primary_is_absent() {
    let selected_style = Style::default().add_modifier(Modifier::REVERSED);
    let row = labeled_row(
        20,
        vec![Span::styled("> ", selected_style)],
        None,
        "agent",
        selected_style,
        true,
        Duration::ZERO,
        Vec::new(),
    );

    assert_eq!(row.spans[0].content.as_ref(), "> ");
    assert_eq!(row.spans[1].content.as_ref(), "");
    assert_eq!(row.spans[2].content.as_ref(), "agent");
    assert_eq!(row.spans[0].style, selected_style);
    assert_eq!(row.spans[2].style, selected_style);
    assert_eq!(
        row.spans
            .iter()
            .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
            .sum::<usize>(),
        7
    );
}

#[test]
fn reserves_indicator_column_when_primary_is_present() {
    let row = labeled_row(
        20,
        vec![Span::raw("> ")],
        Some(Indicator::Merging),
        "agent",
        Style::default(),
        false,
        Duration::ZERO,
        Vec::new(),
    );

    assert_eq!(row.spans[0].content.as_ref(), "> ");
    assert_eq!(row.spans[1].content.as_ref(), "⇄ ");
    assert_eq!(row.spans[2].content.as_ref(), "agent");
    assert_eq!(
        row.spans
            .iter()
            .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
            .sum::<usize>(),
        9
    );
}
