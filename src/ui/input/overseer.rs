use crossterm::event::{KeyCode, KeyEvent};

use crate::{Result, locale::t, model::Selection};

use super::super::{App, Mode, text_input::TextInput};

pub(super) enum PromptAction {
    Stay,
    Cancel,
    Submit(String),
}

pub(super) fn prompt_action(input: &mut TextInput, key: KeyEvent) -> PromptAction {
    match key.code {
        KeyCode::Esc => PromptAction::Cancel,
        KeyCode::Enter if input.text().trim().is_empty() => PromptAction::Stay,
        KeyCode::Enter => PromptAction::Submit(input.text().trim().to_string()),
        _ => {
            input.handle_key(key);
            PromptAction::Stay
        }
    }
}

pub(super) fn handle_normal(app: &mut App, code: KeyCode) -> bool {
    // S kills every live overseer worker, so it is reachable from any row
    // while the overseer panel is active — including worker rows
    // (Selection::Agent), not just the OVERSEER header / category rows. The
    // daemon itself keeps running: merge polling, Discord/MCP commands, and
    // worker monitoring are unaffected, so there is nothing to "turn back on"
    // afterwards (dropr:476).
    if code == KeyCode::Char('S') {
        if !app.overseer_visible {
            return false;
        }
        app.mode = Mode::ConfirmOverseerPanic;
        return true;
    }
    // While the daemon is not running, R starts it — the daemon has no other
    // TUI-reachable start, and reusing R keeps the operator on one key for
    // "the thing overseer needs right now" rather than adding a separate
    // always-visible start key.
    if code == KeyCode::Char('R') {
        if !app.overseer_visible {
            return false;
        }
        if !app.overseer_snapshot.daemon_alive {
            app.start_daemon();
            return true;
        }
        app.show_message(t(app.locale, "overseer daemon is already running"));
        return true;
    }
    // K durably stops the daemon process itself — distinct from S, which only
    // kills workers while leaving the daemon running.
    if code == KeyCode::Char('K') {
        if !app.overseer_visible {
            return false;
        }
        if !app.overseer_snapshot.daemon_alive {
            app.show_message(t(app.locale, "overseer daemon is not running"));
            return true;
        }
        app.mode = Mode::ConfirmDaemonStop;
        return true;
    }
    match app.selected_item() {
        // The control AI row is the one place `i` sends an instruction: it is
        // the row that owns the session the instruction goes into. Not gated
        // on the preview tab — the row has only one (Info), unlike the old
        // Claude-tab gate this replaced.
        Some(Selection::OverseerAi) => {
            if code == KeyCode::Char('i') {
                app.mode = Mode::PromptOverseer {
                    input: TextInput::new(),
                };
                return true;
            }
            false
        }
        Some(Selection::OverseerCategory(category)) => {
            // Clear-all is offered from the category row whether or not it is
            // expanded: the row's own summary already says how many items the
            // operator is about to clear.
            if code == KeyCode::Char('D') && category == crate::model::OverseerCategory::Inbox {
                app.confirm_dismiss_inbox();
                return true;
            }
            false
        }
        Some(Selection::OverseerInbox(index)) => inbox_key(app, index, code),
        _ => false,
    }
}

/// Keys that act on the selected Inbox row. They work from every preview tab —
/// the row lives in the left frame, so what the right pane shows has no say in
/// what acting on it does.
fn inbox_key(app: &mut App, index: usize, code: KeyCode) -> bool {
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            app.approve_inbox(index);
            true
        }
        // `a` answered the inbox before this became a tree row, and it is still
        // the global add-repository key. Claiming it here keeps that muscle
        // memory from opening a clone prompt, and says where answering went.
        KeyCode::Char('a') => {
            app.show_message(t(
                app.locale,
                "press enter to answer the selected inbox item",
            ));
            true
        }
        // Dismissing one row is undoable by hand (delete the entry) and hides a
        // single alert, so it acts immediately. Clearing the list is not, so `D`
        // goes through the same confirmation the other bulk overseer actions do.
        KeyCode::Char('d') => {
            app.dismiss_inbox_item(index);
            true
        }
        KeyCode::Char('D') => {
            app.confirm_dismiss_inbox();
            true
        }
        _ => false,
    }
}

impl App {
    fn response_message(&mut self, result: Result<()>, success: &'static str) {
        match result {
            Ok(()) => self.show_message(t(self.locale, success)),
            Err(error) => self.show_message(error.to_string()),
        }
    }

    /// Panic-stop the overseer: terminate every overseer-managed worker. Runs
    /// synchronously since it is an explicit, operator-initiated action. This
    /// never stops the daemon process itself — see [`App::stop_daemon`] for
    /// that.
    pub(in crate::ui) fn panic_overseer(&mut self) {
        let result = crate::overseer::command::panic_stop_attributed("ui", None);
        self.refresh_overseer_snapshot();
        self.response_message(result, "workers killed; daemon still running");
    }

    /// Durably stop the Overseer daemon process: bootout the launchd service
    /// if one is loaded, else SIGTERM the manually-run process. Runs
    /// synchronously since it is an explicit, operator-initiated action.
    pub(in crate::ui) fn stop_daemon(&mut self) {
        let result = crate::overseer::command::stop_daemon();
        self.refresh_overseer_snapshot();
        match result {
            Ok(crate::overseer::command::StopAttempt::NotRunning) => {
                self.show_message(t(self.locale, "overseer daemon is not running"));
            }
            Ok(crate::overseer::command::StopAttempt::Stopped) => {
                self.show_message(t(self.locale, "overseer daemon stopped"));
            }
            Ok(crate::overseer::command::StopAttempt::StillShuttingDown) => {
                self.show_message(t(
                    self.locale,
                    "overseer daemon shutdown requested; a pass may still be finishing, but it will not restart automatically",
                ));
            }
            Err(error) => self.show_message(error.to_string()),
        }
    }

    /// Start the Overseer daemon: load the launchd service if one is
    /// installed, else report how to run it. Runs synchronously since it is
    /// an explicit, operator-initiated action.
    pub(in crate::ui) fn start_daemon(&mut self) {
        let result = crate::overseer::command::start_daemon();
        self.refresh_overseer_snapshot();
        match result {
            Ok(crate::overseer::command::StartAttempt::NotInstalled) => {
                self.show_message(t(
                    self.locale,
                    "no launchd service installed; install it with `robco service install`, or run `robco daemon` in a terminal",
                ));
            }
            Ok(crate::overseer::command::StartAttempt::Unsupported) => {
                self.show_message(t(
                    self.locale,
                    "launchd service management is unavailable on this OS; run `robco daemon` in a terminal",
                ));
            }
            Ok(crate::overseer::command::StartAttempt::AlreadyRunning) => {
                self.show_message(t(self.locale, "overseer is already running"));
            }
            Ok(crate::overseer::command::StartAttempt::Started) => {
                self.show_message(t(self.locale, "overseer started"));
            }
            Err(error) => self.show_message(error.to_string()),
        }
    }
}

#[cfg(test)]
#[path = "overseer_tests.rs"]
mod tests;
