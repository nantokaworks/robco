use super::*;

fn press(input: &mut TextInput, code: KeyCode) -> bool {
    input.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn ctrl(input: &mut TextInput, ch: char) -> bool {
    input.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL))
}

fn typed(text: &str) -> TextInput {
    let mut input = TextInput::new();
    for ch in text.chars() {
        input.insert(ch);
    }
    input
}

#[test]
fn typing_leaves_the_cursor_after_the_last_character() {
    let input = typed("worker");

    assert_eq!(input.text(), "worker");
    assert_eq!(input.cursor(), 6);
}

#[test]
fn from_string_starts_at_the_end_so_appending_is_unchanged() {
    let input = TextInput::from("request PR".to_string());

    assert_eq!(input.cursor(), input.len());
    assert_eq!(input, *"request PR");
}

#[test]
fn left_then_typing_inserts_at_the_cursor() {
    let mut input = typed("wrker");
    press(&mut input, KeyCode::Left);
    press(&mut input, KeyCode::Left);
    press(&mut input, KeyCode::Left);
    press(&mut input, KeyCode::Left);
    press(&mut input, KeyCode::Char('o'));

    assert_eq!(input.text(), "worker");
    assert_eq!(input.cursor(), 2);
}

#[test]
fn cursor_never_leaves_the_buffer_bounds() {
    let mut input = typed("ab");
    for _ in 0..5 {
        press(&mut input, KeyCode::Left);
    }
    assert_eq!(input.cursor(), 0);

    for _ in 0..5 {
        press(&mut input, KeyCode::Right);
    }
    assert_eq!(input.cursor(), 2);
}

#[test]
fn backspace_at_the_start_and_delete_at_the_end_are_no_ops() {
    let mut input = typed("ab");
    press(&mut input, KeyCode::Home);
    press(&mut input, KeyCode::Backspace);

    assert_eq!(input.text(), "ab");
    assert_eq!(input.cursor(), 0);

    press(&mut input, KeyCode::End);
    press(&mut input, KeyCode::Delete);

    assert_eq!(input.text(), "ab");
    assert_eq!(input.cursor(), 2);
}

#[test]
fn empty_input_survives_every_editing_key() {
    let mut input = TextInput::new();
    for code in [
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::Backspace,
        KeyCode::Delete,
    ] {
        press(&mut input, code);
    }
    ctrl(&mut input, 'w');
    ctrl(&mut input, 'u');

    assert!(input.text().is_empty());
    assert_eq!(input.cursor(), 0);
}

#[test]
fn delete_removes_the_character_under_the_cursor() {
    let mut input = typed("abc");
    press(&mut input, KeyCode::Home);
    press(&mut input, KeyCode::Delete);

    assert_eq!(input.text(), "bc");
    assert_eq!(input.cursor(), 0);
}

#[test]
fn home_and_end_jump_to_the_buffer_edges() {
    let mut input = typed("abc");
    press(&mut input, KeyCode::Home);
    assert_eq!(input.cursor(), 0);

    press(&mut input, KeyCode::End);
    assert_eq!(input.cursor(), 3);
}

#[test]
fn ctrl_a_and_ctrl_e_mirror_home_and_end() {
    let mut input = typed("abc");
    assert!(ctrl(&mut input, 'a'));
    assert_eq!(input.cursor(), 0);

    assert!(ctrl(&mut input, 'e'));
    assert_eq!(input.cursor(), 3);
}

#[test]
fn ctrl_w_deletes_the_word_before_the_cursor() {
    let mut input = typed("one two three");
    ctrl(&mut input, 'w');

    assert_eq!(input.text(), "one two ");
    assert_eq!(input.cursor(), 8);

    ctrl(&mut input, 'w');
    assert_eq!(input.text(), "one ");
    assert_eq!(input.cursor(), 4);
}

#[test]
fn ctrl_w_leaves_the_text_right_of_the_cursor_alone() {
    let mut input = typed("one two");
    press(&mut input, KeyCode::Home);
    press(&mut input, KeyCode::Right);
    press(&mut input, KeyCode::Right);
    press(&mut input, KeyCode::Right);
    ctrl(&mut input, 'w');

    assert_eq!(input.text(), " two");
    assert_eq!(input.cursor(), 0);
}

#[test]
fn ctrl_u_clears_only_the_text_before_the_cursor() {
    let mut input = typed("one two");
    press(&mut input, KeyCode::Left);
    press(&mut input, KeyCode::Left);
    press(&mut input, KeyCode::Left);
    ctrl(&mut input, 'u');

    assert_eq!(input.text(), "two");
    assert_eq!(input.cursor(), 0);
}

#[test]
fn unbound_control_chords_are_not_inserted_as_text() {
    let mut input = typed("abc");

    assert!(!ctrl(&mut input, 'z'));
    assert_eq!(input.text(), "abc");
}

#[test]
fn keys_the_buffer_does_not_own_are_reported_unconsumed() {
    let mut input = typed("abc");

    assert!(!press(&mut input, KeyCode::Enter));
    assert!(!press(&mut input, KeyCode::Esc));
    assert_eq!(input.text(), "abc");
}

#[test]
fn multi_byte_text_edits_mid_string_without_slicing_a_codepoint() {
    let mut input = typed("日本語");
    press(&mut input, KeyCode::Left);
    press(&mut input, KeyCode::Char('の'));

    assert_eq!(input.text(), "日本の語");
    assert_eq!(input.cursor(), 3);

    press(&mut input, KeyCode::Backspace);
    assert_eq!(input.text(), "日本語");
    assert_eq!(input.cursor(), 2);

    press(&mut input, KeyCode::Delete);
    assert_eq!(input.text(), "日本");
    assert_eq!(input.cursor(), 2);
}

#[test]
fn cursor_counts_characters_not_bytes() {
    let input = TextInput::from("日本語");

    assert_eq!(input.len(), 3);
    assert_eq!(input.cursor(), 3);
}
