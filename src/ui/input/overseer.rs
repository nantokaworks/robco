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
    match app.selected_item() {
        Some(Selection::OverseerCategory(_)) => {
            if code == KeyCode::Char('i') && app.preview == PreviewPane::Claude {
                app.mode = Mode::PromptOverseer {
                    input: String::new(),
                };
                return true;
            }
            false
        }
        Some(Selection::OverseerInbox(index)) => inbox_key(app, index, code),
        _ => false,
    }
}

/// Why an inbox item cannot be answered: the escalation is real, but the worker
/// it came from is gone, so there is no session to send an answer into.
const DISPLAY_ONLY: &str = "display-only inbox item: no live session to answer";

/// Keys that act on the selected Inbox row. They work from every preview tab —
/// the row lives in the left frame, so what the right pane shows has no say in
/// what acting on it does.
fn inbox_key(app: &mut App, index: usize, code: KeyCode) -> bool {
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            match app
                .overseer_inbox
                .get(index)
                .and_then(|item| item.target_session.clone())
            {
                Some(session) => app.approve_inbox(&session),
                None => app.show_message(DISPLAY_ONLY),
            }
            true
        }
        // `a` answered the inbox before this became a tree row, and it is still
        // the global add-repository key. Claiming it here keeps that muscle
        // memory from opening a clone prompt, and says where answering went.
        KeyCode::Char('a') => {
            app.show_message("press enter to answer the selected inbox item");
            true
        }
        _ => false,
    }
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
    /// Open the answer prompt for the selected Inbox row, or say why the row
    /// cannot be answered. Never falls through to an attach: an inbox row has no
    /// session of its own to attach to.
    pub(in crate::ui) fn answer_inbox_selected(&mut self, index: usize) {
        let Some(item) = self.overseer_inbox.get(index) else {
            self.show_message("inbox item is no longer listed");
            return;
        };
        let Some(target_session) = item.target_session.clone() else {
            self.show_message(DISPLAY_ONLY);
            return;
        };
        let label = item.label.clone();
        self.mode = Mode::PromptInbox {
            target_session,
            label,
            input: String::new(),
        };
    }

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

    /// Request an overseer dispatch circuit reset: re-enable dispatch now and
    /// clear the daemon-owned failure counter on its next tick.
    pub(in crate::ui) fn reset_overseer(&mut self) {
        let result =
            crate::overseer::command::set_runtime(crate::cli::OverseerSetting::Dispatch, true);
        self.refresh_overseer_snapshot();
        match result {
            Ok(()) if self.overseer_snapshot.daemon_alive => {
                self.show_message(
                    "dispatch circuit reset requested: dispatch on, failures clearing on next tick",
                );
            }
            Ok(()) => self.show_message(format!(
                "dispatch circuit reset requested: dispatch on, failures pending; warning: {}",
                crate::overseer::DISPATCH_WITHOUT_DAEMON_HINT
            )),
            Err(error) => self.show_message(error.to_string()),
        }
    }
}

#[cfg(test)]
#[path = "overseer_tests.rs"]
mod tests;
