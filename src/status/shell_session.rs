use crate::tmux;

use super::proc;
use super::session_alive;

/// Whether the companion `<ai>-shell` TERM session is running a foreground
/// command (anything other than the shell itself). Detected via tmux's
/// `#{pane_current_command}` rather than by scraping output. Returns `false`
/// when the shell session does not exist or the probe fails transiently, so a
/// missing/So-far-unopened TERM never shows a spurious "working" mark.
pub(super) fn shell_session_working(ai_session: &str, panes: Option<&tmux::PaneSnapshot>) -> bool {
    let shell = format!("{ai_session}-shell");
    if session_alive(panes, &shell) != Some(true) {
        return false;
    }
    panes
        .and_then(|panes| panes.pane_current_command(&shell))
        .is_some_and(|cmd| !proc::is_shell_name(cmd))
}
