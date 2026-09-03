use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::Result;

use super::{App, Mode, help};

mod confirm;
mod dropr_task_body;
mod dropr_task_drill;
mod escalation;
mod host_connect;
mod inbox_dismiss;
mod inbox_respond;
mod mouse;
mod normal;
mod overseer;
#[cfg(test)]
pub(in crate::ui) use overseer::remote_chat_target;
mod prompt_agent;
mod prompt_router;
mod tree_nav;

impl App {
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> Result<bool> {
        let server = self.config.tmux_server.clone();
        self.handle_key_with_pr_sender(key, |session, prompt| {
            crate::tmux::send_literal_text(&server, session, prompt)
                .and_then(|()| crate::tmux::send_keys(&server, session, &["Enter"]))
        })
    }
    pub(in crate::ui) fn handle_key_with_pr_sender(
        &mut self,
        key: KeyEvent,
        send: impl FnOnce(&str, &str) -> Result<()>,
    ) -> Result<bool> {
        self.message = None;
        if matches!(key.code, KeyCode::Char('c')) && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Ok(true);
        }
        if let Some(result) = confirm::handle_confirm(self, key) {
            result?;
            self.clamp_selection();
            return Ok(false);
        }
        if let Some(result) = prompt_router::handle(self, key) {
            result?;
            self.clamp_selection();
            return Ok(false);
        }

        match &mut self.mode {
            Mode::PrPrecheck { .. } => {
                if matches!(key.code, KeyCode::Esc) {
                    self.pr_precheck_job = None;
                    self.mode = Mode::Normal;
                }
            }
            Mode::ConfirmPr { .. } => confirm::handle_confirm_pr(self, key, send)?,
            Mode::ErrorDialog { force_kill, .. } => {
                let target = force_kill.clone();
                self.mode = Mode::Normal;
                if matches!(key.code, KeyCode::Char('f') | KeyCode::Char('F'))
                    && let Some(target) = target
                {
                    self.force_kill(target)?;
                }
            }
            // The task-body reading dialog (dropr:501); its own key routing
            // lives in `dropr_task_body` to keep this file under the
            // line-count limit.
            Mode::TaskBody { .. } => dropr_task_body::handle(self, key.code),
            Mode::Help { scroll } => {
                let height = help::terminal_height();
                if help::max_scroll(height) == 0 {
                    self.mode = Mode::Normal;
                } else {
                    match key.code {
                        KeyCode::Down | KeyCode::Char('j') => {
                            *scroll = help::scroll_down(*scroll, height);
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            *scroll = help::scroll_up(*scroll, height);
                        }
                        _ => self.mode = Mode::Normal,
                    }
                }
            }
            // This router only selects the mode family. Keep Normal's guard
            // chain in `normal` in its exact order: the first handler wins.
            Mode::Normal => return normal::handle(self, key),
            Mode::PromptAgent { .. }
            | Mode::PromptRepo { .. }
            | Mode::PromptRenameRepo { .. }
            | Mode::PromptHostConnect { .. }
            | Mode::PromptOverseer { .. }
            | Mode::PromptSession { .. } => unreachable!("handled above"),
            Mode::ConfirmKill { .. }
            | Mode::ConfirmRemoveRepo { .. }
            | Mode::ConfirmMerge { .. }
            | Mode::ConfirmCleanup { .. }
            | Mode::ConfirmDeleteBranch { .. }
            | Mode::ConfirmKillOrphan { .. }
            | Mode::ConfirmOverseerPanic
            | Mode::ConfirmDaemonStop
            | Mode::ConfirmRemoveDiscordChannel { .. }
            | Mode::ConfirmClearChat { .. } => unreachable!("handled above"),
        }

        self.clamp_selection();
        Ok(false)
    }
}
