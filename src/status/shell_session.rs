use crate::tmux;

use super::proc;

/// Whether the companion `<ai>-shell` TERM session is running a foreground
/// command (anything other than the shell itself). Detected via tmux's
/// `#{pane_current_command}` rather than by scraping output. Returns `false`
/// when the shell session does not exist or the probe fails transiently, so a
/// missing/So-far-unopened TERM never shows a spurious "working" mark.
pub(super) fn shell_session_working(ai_session: &str) -> bool {
    let shell = format!("{ai_session}-shell");
    if !matches!(tmux::has_session(&shell), Ok(true)) {
        return false;
    }
    match tmux::pane_current_command(&shell) {
        Ok(Some(cmd)) => !proc::is_shell_name(&cmd),
        _ => false,
    }
}
