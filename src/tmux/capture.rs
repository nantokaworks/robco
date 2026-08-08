use std::process::Command;

use crate::Result;

use super::{command_output, session::exact};

pub fn capture_plain(session: &str) -> Result<String> {
    let output = Command::new("tmux")
        .args(["capture-pane", "-e", "-p", "-t", &exact(session)])
        .output()?;
    command_output(output, "tmux capture-pane")
}

/// Capture one screenful (`height` rows) of the pane, starting `offset` lines
/// back into scrollback history. `offset == 0` is the live screen.
pub fn capture_scrollback(session: &str, offset: u16, height: u16) -> Result<String> {
    if offset == 0 {
        return capture_plain(session);
    }
    let start = -i64::from(offset);
    let end = start + i64::from(height.saturating_sub(1));
    let output = Command::new("tmux")
        .args([
            "capture-pane",
            "-e",
            "-p",
            "-S",
            &start.to_string(),
            "-E",
            &end.to_string(),
            "-t",
            &exact(session),
        ])
        .output()?;
    command_output(output, "tmux capture-pane")
}

/// Lines of scrollback history the pane currently holds (`#{history_size}`).
pub fn history_size(session: &str) -> Result<u16> {
    let output = Command::new("tmux")
        .args([
            "display-message",
            "-p",
            "-t",
            &exact(session),
            "#{history_size}",
        ])
        .output()?;
    let value = command_output(output, "tmux display-message")?;
    let size = value.trim().parse::<u32>().unwrap_or(0);
    Ok(size.min(u32::from(u16::MAX)) as u16)
}

pub fn capture_text(session: &str) -> Result<String> {
    let output = Command::new("tmux")
        .args(["capture-pane", "-p", "-t", &exact(session)])
        .output()?;
    command_output(output, "tmux capture-pane")
}

/// PID of the process tmux started for the session's active pane.
pub fn pane_pid(session: &str) -> Result<Option<u32>> {
    let output = Command::new("tmux")
        .args([
            "display-message",
            "-p",
            "-t",
            &exact(session),
            "#{pane_pid}",
        ])
        .output()?;
    let value = command_output(output, "tmux display-message")?;
    Ok(value.trim().parse().ok())
}
