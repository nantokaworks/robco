use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::{config::Config, registry::Registry};

fn test_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    App::new(Registry::default(), Config::default(), temp.path().into())
}

#[test]
fn overseer_panes_show_info_then_claude() {
    assert_eq!(
        panes_for(Some(Selection::Overseer)),
        &[PreviewPane::Info, PreviewPane::Claude]
    );
}

#[test]
fn overseer_instruction_key_opens_prompt() {
    let mut app = test_app();
    app.overseer_visible = true;
    app.selected = 0;
    app.preview = PreviewPane::Claude;

    app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
        .unwrap();

    assert!(matches!(app.mode, Mode::PromptOverseer { .. }));
}

#[test]
fn visible_message_does_not_swallow_next_key() {
    let mut app = test_app();
    app.show_message("done");
    let quit = app
        .handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE))
        .unwrap();

    assert!(!quit);
    assert!(app.message.is_none());
    assert!(matches!(app.mode, Mode::Help { scroll: 0 }));
}

#[test]
fn confirm_pr_y_and_n_edit_and_escape_cancels() {
    let mut app = test_app();
    app.mode = Mode::ConfirmPr {
        repo_path: "/repo".into(),
        agent_id: "agent".to_string(),
        branch: "feature/agent".to_string(),
        input: "prompt".to_string(),
    };

    for code in [KeyCode::Char('y'), KeyCode::Char('n'), KeyCode::Backspace] {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
            .unwrap();
    }
    assert!(matches!(
        &app.mode,
        Mode::ConfirmPr { input, .. } if input == "prompty"
    ));

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    assert!(matches!(app.mode, Mode::Normal));
}
