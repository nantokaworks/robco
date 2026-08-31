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
        let session = agent::repo_claude_session_name(&self.config.tmux_session_prefix, &repo_node);
        if repo_node.host.is_some() {
            let repo = self
                .registry
                .repos
                .iter()
                .position(|repo| repo.path == repo_node.path && repo.host == repo_node.host)
                .expect("selected repo remains present");
            let Some(client) = self.remote_client_for_repo(repo) else {
                self.show_message(t(self.locale, "remote host is not connected"));
                return;
            };
            match client.clear_chat(&repo_node.path.display().to_string()) {
                Ok(outcome) if outcome.ok => self.show_message(fmt(
                    self.locale,
                    "cleared chat session for {}",
                    &[&repo_node.name],
                )),
                Ok(_) => self.show_message(t(self.locale, "remote clear was refused")),
                Err(error) => self.show_message(error.to_string()),
            }
            return;
        }
        let clear_command = self
            .config
            .default_program_clear_command()
            .expect("clear_chat_blocker already confirmed a command is configured");
        let result = tmux::send_literal_text(&self.config.tmux_server, &session, &clear_command)
            .and_then(|()| tmux::send_keys(&self.config.tmux_server, &session, &["Enter"]));
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
        if repo_node.host.is_some() {
            return (!matches!(
                repo_node.main_status,
                Some(Status::Idle) | Some(Status::Done)
            ))
            .then(|| {
                t(
                    self.locale,
                    "chat session is busy — wait for it to finish before clearing",
                )
                .to_string()
            });
        }
        let Some(_clear_command) = self.config.default_program_clear_command() else {
            return Some(fmt(
                self.locale,
                "no clear command configured for {}",
                &[&self.config.default_program_command()],
            ));
        };
        let session = agent::repo_claude_session_name(&self.config.tmux_session_prefix, repo_node);
        match tmux::has_session(&self.config.tmux_server, &session) {
            Ok(true) => {}
            Ok(false) => return Some(t(self.locale, "no live chat session to clear").to_string()),
            Err(err) if tmux_binary_missing(&err) => {
                return Some(t(self.locale, "tmux is not installed, or not on PATH").to_string());
            }
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

/// Whether `error` is a raw `ErrorKind::NotFound` from spawning `tmux`
/// itself, rather than a normal tmux failure. Uncaught, that error reads as
/// "io error: No such file or directory (os error 2)" to an operator —
/// meaningless unless they already know that's what a missing binary looks
/// like on this platform (GitHub's `macos-latest` runner has no `tmux`, per
/// dropr:550's CI failure). Named the real cause instead, the same way
/// `repo_watch_advisory::probe` distinguishes a missing tool from one that
/// ran and failed.
fn tmux_binary_missing(error: &crate::Error) -> bool {
    matches!(error, crate::Error::Io(io_err) if io_err.kind() == std::io::ErrorKind::NotFound)
}

#[cfg(test)]
#[path = "clear_chat_tests.rs"]
mod tests;
