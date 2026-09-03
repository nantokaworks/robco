use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::{config::Config, registry::Registry};

#[test]
fn newline_chords_insert_without_submitting() {
    for key in [
        KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT),
        KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT),
        KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
    ] {
        let mut input = TextInput::from("line one");

        let action = instruction_prompt_action(&mut input, key);

        assert!(matches!(action, PromptAction::Stay));
        assert_eq!(input.text(), "line one\n");
    }
}

#[test]
fn plain_enter_submits_multiline_text() {
    let mut input = TextInput::from("  line one\nline two  ");

    let action = instruction_prompt_action(
        &mut input,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    );

    assert!(matches!(action, PromptAction::Submit(text) if text == "line one\nline two"));
}

#[test]
fn blank_multiline_instruction_stays_open() {
    let mut input = TextInput::from(" \n  ");

    let action = instruction_prompt_action(
        &mut input,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    );

    assert!(matches!(action, PromptAction::Stay));
}

#[test]
fn ordinary_prompt_ignores_newline_chords() {
    let mut input = TextInput::from("rename");

    let action = prompt_action(&mut input, KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));

    assert!(matches!(action, PromptAction::Stay));
    assert_eq!(input.text(), "rename");
}

#[test]
fn rename_prompt_ignores_alt_enter() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.mode = Mode::PromptRenameRepo {
        path: "/repo".into(),
        input: TextInput::from("renamed"),
    };

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT))
        .unwrap();

    assert!(matches!(
        &app.mode,
        Mode::PromptRenameRepo { input, .. } if input.text() == "renamed"
    ));
}

#[test]
fn an_unrecognized_enter_chord_inserts_a_newline_instead_of_submitting() {
    let mut input = TextInput::from("half written");
    let action = instruction_prompt_action(
        &mut input,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
    );
    assert!(matches!(action, PromptAction::Stay));
    assert_eq!(input.text(), "half written\n");

    let action = instruction_prompt_action(
        &mut input,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL | KeyModifiers::SHIFT),
    );
    assert!(matches!(action, PromptAction::Stay));
    assert_eq!(input.text(), "half written\n\n");
}
