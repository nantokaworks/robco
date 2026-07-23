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
    // Stop is an overseer-wide action, so it is reachable from any row while
    // the overseer panel is active — including worker rows (Selection::Agent),
    // not just the OVERSEER header / category rows. This keeps the always-on
    // [S] STOP footer hint honest. The ConfirmOverseerPanic dialog still gates
    // the destructive step, and it works from any preview tab.
    if code == KeyCode::Char('S') {
        if app.overseer_visible {
            app.mode = Mode::ConfirmOverseerPanic;
            return true;
        }
        return false;
    }
    if code == KeyCode::Char('R') {
        if !app.overseer_visible {
            return false;
        }
        if app.overseer_snapshot.circuit_open() {
            app.mode = Mode::ConfirmOverseerReset;
            return true;
        }
        app.show_message("circuit is closed; nothing to reset");
        return true;
    }
    if !matches!(
        app.selected_item(),
        Some(Selection::Overseer | Selection::OverseerCategory(_))
    ) {
        return false;
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
        self.refresh_overseer_snapshot();
        self.response_message(result, "overseer stopped: dispatch off, workers killed");
    }

    /// Reset the overseer dispatch circuit: re-enable dispatch and clear the
    /// failure counter. Runs synchronously as an explicit operator action.
    pub(in crate::ui) fn reset_overseer(&mut self) {
        let result =
            crate::overseer::command::set_runtime(crate::cli::OverseerSetting::Dispatch, true);
        self.refresh_overseer_snapshot();
        self.response_message(
            result,
            "dispatch circuit reset: dispatch on, failures cleared",
        );
    }
}

#[cfg(test)]
#[path = "overseer_tests.rs"]
mod tests;
