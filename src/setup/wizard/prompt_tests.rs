use std::io::Cursor;

use super::prompt;

#[test]
fn text_and_confirm_use_defaults() {
    let mut input = Cursor::new(b"\n\n");
    let mut output = Vec::new();
    assert_eq!(
        prompt::text(&mut input, &mut output, "name", "robco").unwrap(),
        "robco"
    );
    assert!(prompt::confirm(&mut input, &mut output, "enabled", true).unwrap());
}

#[test]
fn confirm_retries_after_garbage() {
    let mut input = Cursor::new(b"maybe\nn\n");
    let mut output = Vec::new();
    assert!(!prompt::confirm(&mut input, &mut output, "enabled", true).unwrap());
    assert!(String::from_utf8(output).unwrap().contains("enter y or n"));
}

#[test]
fn select_and_number_retry_invalid_values() {
    let mut input = Cursor::new(b"wat\n9\n2\nnope\n4\n");
    let mut output = Vec::new();
    let choices = vec!["one".into(), "two".into()];
    assert_eq!(
        prompt::select(&mut input, &mut output, "pick", &choices, 0).unwrap(),
        1
    );
    assert_eq!(
        prompt::number(&mut input, &mut output, "count", 3, 0, 5).unwrap(),
        4
    );
}

#[test]
fn secret_text_reads_the_answer_without_a_default() {
    let mut input = Cursor::new(b"typed-secret\n");
    let mut output = Vec::new();
    assert_eq!(
        prompt::secret_text(&mut input, &mut output, "token").unwrap(),
        "typed-secret"
    );

    let mut empty = Cursor::new(b"\n");
    assert_eq!(
        prompt::secret_text(&mut empty, &mut Vec::new(), "token").unwrap(),
        ""
    );

    let mut eof = Cursor::new(Vec::<u8>::new());
    assert!(prompt::secret_text(&mut eof, &mut Vec::new(), "token").is_err());
}

#[test]
fn validated_text_retries_and_eof_is_an_error() {
    let mut input = Cursor::new(b"bad\n123\n");
    let mut output = Vec::new();
    let answer = prompt::validated_text(&mut input, &mut output, "id", "", "digits", |value| {
        value.bytes().all(|byte| byte.is_ascii_digit())
    })
    .unwrap();
    assert_eq!(answer, "123");

    let mut eof = Cursor::new(Vec::<u8>::new());
    assert!(prompt::text(&mut eof, &mut Vec::new(), "x", "y").is_err());
}
