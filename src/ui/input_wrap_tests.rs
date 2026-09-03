use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;

fn rendered(wrapped: &WrappedInput) -> Vec<String> {
    wrapped.lines.iter().map(ToString::to_string).collect()
}

fn press(input: &mut TextInput, code: KeyCode) {
    input.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

#[test]
fn wraps_words_to_available_width() {
    let wrapped = input_lines("prompt", &"one two three four".into(), 16, 10);

    assert_eq!(
        rendered(&wrapped),
        [" prompt: one two", "          three ", "         four_"]
    );
    assert!(wrapped.lines.iter().all(|line| line.width() <= 16));
}

#[test]
fn wraps_cjk_without_splitting_characters() {
    let wrapped = input_lines("prompt", &"日本語入力".into(), 14, 10);

    assert_eq!(
        rendered(&wrapped),
        [" prompt: 日本", "         語入", "         力_"]
    );
    assert!(wrapped.lines.iter().all(|line| line.width() <= 14));
}

#[test]
fn tail_scroll_keeps_cursor_visible() {
    let wrapped = input_lines("prompt", &"one two three four five".into(), 16, 2);

    assert_eq!(wrapped.lines.len(), 2);
    assert_eq!(wrapped.lines[0].to_string(), " prompt: four ");
    assert_eq!(wrapped.lines[1].to_string(), "         five_");
}

#[test]
fn narrow_widths_keep_lines_bounded_and_cursor_visible() {
    for max_width in 0..=9 {
        let wrapped = input_lines("prompt", &"日本".into(), max_width, 10);

        assert!(wrapped.lines.iter().all(|line| line.width() <= max_width));
        if max_width > 0 {
            assert!(wrapped.lines.last().unwrap().to_string().ends_with('_'));
        }
    }
}

#[test]
fn preserves_whitespace_and_trailing_cursor_position() {
    let wrapped = input_lines("prompt", &"  one  two  ".into(), 30, 10);

    assert_eq!(rendered(&wrapped), [" prompt:   one  two  _"]);
}

#[test]
fn wrapping_does_not_normalize_whitespace() {
    let wrapped = wrap_text(" one   two ", 6);

    assert_eq!(
        wrapped
            .iter()
            .map(|(line, _)| line.as_str())
            .collect::<String>(),
        " one   two "
    );
    assert!(wrapped.iter().all(|(line, _)| width(line) <= 6));
}

#[test]
fn wrapped_line_starts_track_character_offsets() {
    let wrapped = wrap_text("one two three", 7);

    let mut offset = 0;
    for (line, start) in &wrapped {
        assert_eq!(*start, offset);
        offset += line.chars().count();
    }
}

#[test]
fn caret_sits_after_the_last_character_by_default() {
    let wrapped = input_lines("prompt", &"one tw".into(), 16, 10);

    // Row 0 is the only row; the caret column is the 9-column label plus the
    // six columns of text before it.
    assert_eq!(wrapped.caret, (0, 15));
    assert_eq!(rendered(&wrapped), [" prompt: one tw_"]);
}

#[test]
fn a_full_last_line_gives_the_trailing_caret_a_row_of_its_own() {
    let wrapped = input_lines("prompt", &"one two".into(), 16, 10);

    // "one two" fills the 7 columns left over after the label, so the caret
    // cannot share that row.
    assert_eq!(rendered(&wrapped), [" prompt: one two", "         _"]);
    assert_eq!(wrapped.caret, (1, 9));
}

#[test]
fn caret_follows_the_cursor_back_onto_an_earlier_row() {
    let mut input = TextInput::from("one two three four");
    for _ in 0..12 {
        press(&mut input, KeyCode::Left);
    }
    let wrapped = input_lines("prompt", &input, 16, 10);

    // The cursor sits on the "o" of "two": row 0, after the 9-column label and
    // the six columns of "one tw".
    assert_eq!(wrapped.caret, (0, 15));
    assert_eq!(rendered(&wrapped)[0], " prompt: one two");
}

#[test]
fn caret_column_counts_display_columns_for_wide_glyphs() {
    let mut input = TextInput::from("日本語");
    press(&mut input, KeyCode::Left);
    let wrapped = input_lines("prompt", &input, 30, 10);

    // Label is 9 columns; two full-width glyphs precede the caret.
    assert_eq!(wrapped.caret, (0, 13));
}

#[test]
fn window_scrolls_up_to_keep_an_early_caret_visible() {
    let mut input = TextInput::from("one two three four five");
    press(&mut input, KeyCode::Home);
    let wrapped = input_lines("prompt", &input, 16, 2);

    assert_eq!(wrapped.lines.len(), 2);
    assert_eq!(wrapped.lines[0].to_string(), " prompt: one two");
    assert_eq!(wrapped.caret, (0, 9));
}

#[test]
fn mid_string_caret_does_not_widen_the_line() {
    let mut input = TextInput::from("one two");
    press(&mut input, KeyCode::Home);
    let wrapped = input_lines("prompt", &input, 16, 10);

    assert_eq!(rendered(&wrapped), [" prompt: one two"]);
    assert_eq!(wrapped.caret, (0, 9));
}

#[test]
fn hard_breaks_create_lines_and_preserve_offsets() {
    let wrapped = wrap_text("one\ntwo\n", 20);

    assert_eq!(
        wrapped,
        vec![
            ("one".to_string(), 0),
            ("two".to_string(), 4),
            (String::new(), 8),
        ]
    );
}

#[test]
fn caret_tracks_hard_breaks_and_soft_wraps_together() {
    let input = TextInput::from("one two\nthree four");
    let wrapped = input_lines("prompt", &input, 16, 10);

    assert_eq!(
        rendered(&wrapped),
        [" prompt: one two", "         three ", "         four_"]
    );
    assert_eq!(wrapped.caret, (2, 13));
}
