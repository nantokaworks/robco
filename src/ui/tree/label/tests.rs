use std::time::Duration;

use ratatui::{
    style::{Modifier, Style},
    text::Span,
};
use unicode_width::UnicodeWidthStr;

use super::*;

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
fn rows_with_variation_selector_titles_never_overflow_their_row_width() {
    // Real board titles (dropr:481): `♻️` and `⚡️` are a base scalar plus
    // U+FE0F, drawn at 2 columns by a terminal though the base scalar alone
    // measures 1; `🐛` is already a single 2-column scalar. `row_width` is
    // tight enough that every title here must be truncated, which is
    // exactly the path a char-by-char width scan used to undercount —
    // producing a row one column wider than `row_width` allowed, so the
    // `right` span (a trailing task number, same as a board row's own
    // `#481`) landed one column later than every other row.
    let row_width = 20;
    let row = |title: &str| {
        labeled_row(
            row_width,
            vec![Span::raw("> ")],
            None,
            title,
            Style::default(),
            false,
            Duration::ZERO,
            vec![Span::raw(" #481")],
        )
    };

    for title in [
        "♻️ [api] Retire the polling dispatcher",
        "⚡️ [web] Add user invitation feature",
        "🐛 [ui] Say which repository an Inbox row is about",
    ] {
        let line = row(title);
        let total = line
            .spans
            .iter()
            .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
            .sum::<usize>();
        assert!(
            total <= usize::from(row_width),
            "title {title:?} produced a {total}-column row, wider than row_width {row_width}"
        );
        // The trailing task number always survives intact: alignment means
        // the title gave way to it, never the other way round.
        assert_eq!(line.spans.last().unwrap().content.as_ref(), " #481");
    }
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
