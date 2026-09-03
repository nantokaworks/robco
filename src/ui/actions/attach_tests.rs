use std::cell::RefCell;

use crate::{config::Config, registry::Registry};

use super::*;

fn test_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    App::new(Registry::default(), Config::default(), temp.path().into())
}

fn tmux_failure() -> crate::Error {
    crate::Error::Command {
        context: "tmux",
        stderr: "boom".into(),
    }
}

#[test]
fn instruct_session_sends_the_multiline_instruction_then_enter() {
    let mut app = test_app();
    let calls = RefCell::new(Vec::new());

    app.instruct_session_with(
        "target",
        "line one\nline two",
        |session, text| {
            calls.borrow_mut().push(format!("literal:{session}:{text}"));
            Ok(())
        },
        |session, keys| {
            calls
                .borrow_mut()
                .push(format!("keys:{session}:{}", keys.join(",")));
            Ok(())
        },
    );

    assert_eq!(
        calls.borrow().as_slice(),
        ["literal:target:line one\nline two", "keys:target:Enter"]
    );
    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("instruction sent")
    );
}

#[test]
fn a_failed_send_reports_the_error_and_sends_no_enter() {
    let mut app = test_app();
    let calls = RefCell::new(Vec::new());

    app.instruct_session_with(
        "target",
        "go",
        |_, _| Err(tmux_failure()),
        |session, keys| {
            calls
                .borrow_mut()
                .push(format!("keys:{session}:{}", keys.join(",")));
            Ok(())
        },
    );

    assert!(
        calls.borrow().is_empty(),
        "Enter must not follow a failed send"
    );
    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("tmux failed: boom")
    );
}
