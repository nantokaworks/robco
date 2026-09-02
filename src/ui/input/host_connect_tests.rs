use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::{config::Config, registry::Registry};

fn test_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    App::new(Registry::default(), Config::default(), temp.path().into())
}

fn press(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
        .unwrap();
}

#[test]
fn h_opens_the_prompt_and_escape_closes_it_without_adding_a_host() {
    let mut app = test_app();

    press(&mut app, KeyCode::Char('H'));
    assert!(matches!(app.mode, Mode::PromptHostConnect { .. }));

    press(&mut app, KeyCode::Esc);
    assert!(matches!(app.mode, Mode::Normal));
    assert!(app.hosts.is_empty());
}

#[test]
fn submit_adds_one_trimmed_host_and_reports_connecting() {
    let mut app = test_app();
    app.mode = Mode::PromptHostConnect {
        input: TextInput::from("  -V  "),
    };

    press(&mut app, KeyCode::Enter);

    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(app.hosts.len(), 1);
    assert_eq!(app.hosts[0].label.ssh, "-V");
    assert_eq!(app.hosts[0].label.name, "-V");
    assert_eq!(
        app.message.as_ref().map(|value| value.0.as_str()),
        Some("connecting to -V")
    );
}

#[test]
fn duplicate_submit_adds_nothing_and_reports_the_existing_host() {
    let mut app = test_app();
    app.connect_host("-V".into());
    app.mode = Mode::PromptHostConnect {
        input: TextInput::from(" -V "),
    };

    press(&mut app, KeyCode::Enter);

    assert_eq!(app.hosts.len(), 1);
    assert_eq!(
        app.message.as_ref().map(|value| value.0.as_str()),
        Some("already connected to -V")
    );
}

#[test]
fn empty_submit_keeps_the_prompt_open() {
    let mut app = test_app();
    app.mode = Mode::PromptHostConnect {
        input: TextInput::from("   "),
    };

    press(&mut app, KeyCode::Enter);

    assert!(matches!(app.mode, Mode::PromptHostConnect { .. }));
    assert!(app.hosts.is_empty());
}
