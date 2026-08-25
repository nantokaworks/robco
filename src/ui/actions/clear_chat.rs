//! `C` on a repo row: send the profile's configured clear command to that
//! repository's own main-worktree chat session (`agent::repo_claude_session_name`)
//! — the one `Selection::Repo(usize)` owns (`src/model.rs`). Scoped to that one
//! session on purpose: a worker agent's own session is mid-task by definition,
//! so `C` does nothing to it, and this action never reaches for one.
//!
//! Confirmed like `checkout_main.rs`'s `c` sibling is not — clearing discards
//! the whole conversation with no way back, so it goes through
//! `Mode::ConfirmClearChat` the same way `x` (kill) and `m` (merge) do.

use std::path::Path;

use crate::{
    agent,
    locale::{fmt, t},
    model::{Selection, Status},
    tmux,
};

use super::super::{App, Mode};

impl App {
    pub(in crate::ui) fn clear_chat_selected(&mut self) {
        let Some(Selection::Repo(repo)) = self.selected_item() else {
            self.show_message(t(self.locale, "C: select a repo to clear its chat session"));
            return;
        };
        let Some(repo_node) = self.registry.repos.get(repo).cloned() else {
            return;
        };
        if let Some(message) = self.clear_chat_blocker(&repo_node) {
            self.show_message(message);
            return;
        }
        self.mode = Mode::ConfirmClearChat {
            path: repo_node.path,
        };
    }

    pub(in crate::ui) fn clear_chat_confirmed(&mut self, path: &Path) {
        let Some(repo_node) = self
            .registry
            .repos
            .iter()
            .find(|repo| repo.path == path)
            .cloned()
        else {
            self.show_message(t(self.locale, "repository changed, not cleared"));
            return;
        };
        if let Some(message) = self.clear_chat_blocker(&repo_node) {
            self.show_message(message);
            return;
        }
        let clear_command = self
            .config
            .default_program_clear_command()
            .expect("clear_chat_blocker already confirmed a command is configured");
        let session = agent::repo_claude_session_name(&self.config.tmux_session_prefix, &repo_node);
        let result = tmux::send_literal_text(&session, &clear_command)
            .and_then(|()| tmux::send_keys(&session, &["Enter"]));
        match result {
            Ok(()) => self.show_message(fmt(
                self.locale,
                "cleared chat session for {}",
                &[&repo_node.name],
            )),
            Err(err) => self.show_message(err.to_string()),
        }
    }

    /// Shared precondition for both the keypress and the confirm step: a
    /// configured clear command, a live session, and a session that is not
    /// mid-turn or holding a prompt open — clearing over either would discard
    /// live work in progress rather than a settled conversation, the same
    /// concern `report.rs`'s `guard_delivery` guards typing into a session
    /// for. Returns the localized refusal message when the clear must not
    /// proceed.
    fn clear_chat_blocker(&self, repo_node: &crate::model::RepoNode) -> Option<String> {
        let Some(_clear_command) = self.config.default_program_clear_command() else {
            return Some(fmt(
                self.locale,
                "no clear command configured for {}",
                &[&self.config.default_program_command()],
            ));
        };
        let session = agent::repo_claude_session_name(&self.config.tmux_session_prefix, repo_node);
        match tmux::has_session(&session) {
            Ok(true) => {}
            Ok(false) => return Some(t(self.locale, "no live chat session to clear").to_string()),
            Err(err) => return Some(err.to_string()),
        }
        if !matches!(
            repo_node.main_status,
            Some(Status::Idle) | Some(Status::Done)
        ) {
            return Some(
                t(
                    self.locale,
                    "chat session is busy — wait for it to finish before clearing",
                )
                .to_string(),
            );
        }
        None
    }
}

#[cfg(test)]
#[path = "clear_chat_tests.rs"]
mod tests;
