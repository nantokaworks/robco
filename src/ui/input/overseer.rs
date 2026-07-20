use crossterm::event::{KeyCode, KeyEvent};

use crate::{Result, model::Selection};

use super::super::{App, Mode, PreviewPane};

pub(super) enum PromptAction {
    Stay,
    Cancel,
    Submit(String),
}

pub(super) fn prompt_action(input: &mut String, key: KeyEvent) -> PromptAction {
    match key.code {
        KeyCode::Esc => PromptAction::Cancel,
        KeyCode::Enter if input.trim().is_empty() => PromptAction::Stay,
        KeyCode::Enter => PromptAction::Submit(input.trim().to_string()),
        KeyCode::Backspace => {
            input.pop();
            PromptAction::Stay
        }
        KeyCode::Char(ch) => {
            input.push(ch);
            PromptAction::Stay
        }
        _ => PromptAction::Stay,
    }
}

pub(super) fn handle_normal(app: &mut App, code: KeyCode) -> bool {
    if !matches!(
        app.selected_item(),
        Some(Selection::Overseer | Selection::OverseerCategory(_))
    ) {
        return false;
    }
    // Stop works from any preview tab so the operator can always reach it.
    if code == KeyCode::Char('S') {
        app.mode = Mode::ConfirmOverseerPanic;
        return true;
    }
    if code == KeyCode::Char('i') && app.preview == PreviewPane::Claude {
        app.mode = Mode::PromptOverseer {
            input: String::new(),
        };
        return true;
    }
    if app.preview != PreviewPane::Info {
        return false;
    }
    match code {
        KeyCode::Char('[') => {
            app.overseer_inbox_selected = app.overseer_inbox_selected.saturating_sub(1);
        }
        KeyCode::Char(']') => {
            app.overseer_inbox_selected =
                (app.overseer_inbox_selected + 1).min(app.overseer_inbox.len().saturating_sub(1));
        }
        KeyCode::Char('a') => {
            if let Some(item) = app.overseer_inbox.get(app.overseer_inbox_selected) {
                if let Some(target_session) = item.target_session.clone() {
                    app.mode = Mode::PromptInbox {
                        target_session,
                        label: item.label.clone(),
                        input: String::new(),
                    };
                } else {
                    app.show_message("inbox item is display-only");
                }
            } else {
                app.show_message("overseer inbox is empty");
            }
        }
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            if let Some(session) = app
                .overseer_inbox
                .get(app.overseer_inbox_selected)
                .and_then(|item| item.target_session.clone())
            {
                app.approve_inbox(&session);
            } else {
                app.show_message("overseer inbox is empty");
            }
        }
        _ => return false,
    }
    true
}

pub(super) enum InboxResponse<'a> {
    Answer(&'a str),
    Approve,
}

pub(super) fn send_response(
    session: &str,
    response: InboxResponse<'_>,
    mut literal: impl FnMut(&str, &str) -> Result<()>,
    mut keys: impl FnMut(&str, &[&str]) -> Result<()>,
) -> Result<()> {
    match response {
        InboxResponse::Answer(text) => {
            literal(session, text)?;
            keys(session, &["Enter"])
        }
        InboxResponse::Approve => keys(session, &["y", "Enter"]),
    }
}

impl App {
    pub(super) fn answer_inbox(&mut self, session: &str, answer: &str) {
        let result = send_response(
            session,
            InboxResponse::Answer(answer),
            crate::tmux::send_literal_text,
            crate::tmux::send_keys,
        );
        self.response_message(result, "answer sent");
    }

    fn approve_inbox(&mut self, session: &str) {
        let result = send_response(
            session,
            InboxResponse::Approve,
            crate::tmux::send_literal_text,
            crate::tmux::send_keys,
        );
        self.response_message(result, "approval sent");
    }

    fn response_message(&mut self, result: Result<()>, success: &str) {
        match result {
            Ok(()) => self.show_message(success),
            Err(error) => self.show_message(error.to_string()),
        }
    }

    /// Panic-stop the overseer: disable dispatch and terminate every
    /// overseer-managed worker. Runs synchronously since it is an explicit,
    /// operator-initiated action.
    pub(in crate::ui) fn panic_overseer(&mut self) {
        let result = crate::overseer::command::panic_stop_attributed("ui", None);
        self.response_message(result, "overseer stopped: dispatch off, workers killed");
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::{config::Config, model::OverseerCategory, registry::Registry};

    #[test]
    fn enter_submits_trimmed_instruction() {
        let mut input = "  review task  ".to_string();
        let action = prompt_action(
            &mut input,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
        assert!(matches!(action, PromptAction::Submit(text) if text == "review task"));
    }

    #[test]
    fn answer_and_approve_use_existing_tmux_sequences() {
        let calls = RefCell::new(Vec::new());
        send_response(
            "target",
            InboxResponse::Answer("ship it"),
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
        )
        .unwrap();
        assert_eq!(
            calls.borrow().as_slice(),
            ["literal:target:ship it", "keys:target:Enter"]
        );

        calls.borrow_mut().clear();
        send_response(
            "target",
            InboxResponse::Approve,
            |_, _| Ok(()),
            |session, keys| {
                calls
                    .borrow_mut()
                    .push(format!("keys:{session}:{}", keys.join(",")));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(calls.borrow().as_slice(), ["keys:target:y,Enter"]);
    }

    #[test]
    fn inbox_navigation_is_handled_from_category_selection() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
        app.overseer_visible = true;
        app.selected = OverseerCategory::Inbox.index() + 1;
        app.preview = PreviewPane::Info;

        assert!(handle_normal(&mut app, KeyCode::Char('[')));
        assert!(handle_normal(&mut app, KeyCode::Char(']')));
    }

    #[test]
    fn stop_key_opens_panic_confirm_from_any_overseer_tab() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
        app.overseer_visible = true;
        app.selected = 0; // OVERSEER root row.
        // Works regardless of the active preview tab.
        app.preview = PreviewPane::Claude;

        assert!(handle_normal(&mut app, KeyCode::Char('S')));
        assert!(matches!(app.mode, Mode::ConfirmOverseerPanic));
    }

    #[test]
    fn stop_key_is_ignored_without_an_overseer_selection() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
        app.overseer_visible = false;

        assert!(!handle_normal(&mut app, KeyCode::Char('S')));
        assert!(matches!(app.mode, Mode::Normal));
    }
}
