use std::time::Duration;

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
fn truncates_a_variation_selector_title_without_overflowing_its_budget() {
    // A real board title (dropr:481): `♻️` is `U+267B` plus `U+FE0F`, drawn
    // at 2 columns by a terminal though `U+267B` alone measures 1. Before
    // the fix, `prefix_within`'s char-by-char scan priced the pair at 1
    // column, so it kept one extra character and the truncated string
    // (content + `…`) came out a column wider than `available` allowed —
    // exactly the "row drawn one column narrower than the terminal
    // renders it" bug this covers.
    let title = "♻️ [api] Retire the polling dispatcher";
    let available = 13;
    let result = display(title, available, false, Duration::ZERO);
    assert_eq!(result, "♻️ [api] Ret…");
    assert_eq!(UnicodeWidthStr::width(result.as_str()), available);
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
