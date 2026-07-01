use std::process::Command;

use crate::Result;

use super::{command_output, session::exact};

pub fn capture_plain(session: &str) -> Result<String> {
    let output = Command::new("tmux")
        .args(["capture-pane", "-e", "-p", "-t", &exact(session)])
        .output()?;
    command_output(output, "tmux capture-pane")
}

pub fn capture_text(session: &str) -> Result<String> {
    let output = Command::new("tmux")
        .args(["capture-pane", "-p", "-t", &exact(session)])
        .output()?;
    command_output(output, "tmux capture-pane")
}

/// The foreground process running in the session's active pane (tmux's
/// `#{pane_current_command}`). When the pane sits at the shell prompt this is
/// the shell itself (`zsh` / `bash` / …); while a command runs it is that
/// command's name. Used to tell whether the companion TERM (shell) session is
/// busy without scraping its output. A missing session yields `Ok(None)`
/// (`display-message` exits 0 and prints an empty string for one), so a real
/// probe failure stays distinguishable as `Err`.
pub fn pane_current_command(session: &str) -> Result<Option<String>> {
    let output = Command::new("tmux")
        .args([
            "display-message",
            "-p",
            "-t",
            &exact(session),
            "#{pane_current_command}",
        ])
        .output()?;
    let value = command_output(output, "tmux display-message")?;
    let trimmed = value.trim();
    Ok((!trimmed.is_empty()).then(|| trimmed.to_string()))
}
