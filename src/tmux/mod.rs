mod attach;
mod capture;
mod env;
mod keys;
mod launch;
mod panes;
mod resize;
mod session;

pub use attach::{attach, send_keys, send_literal_text, single_line};
pub use capture::{capture_plain, capture_scrollback, capture_text, history_size, pane_pid};
pub use env::session_env;
pub use launch::new_worker_session;
pub use panes::{PaneSnapshot, capture_panes};
pub use resize::resize_session;
#[cfg(test)]
pub(crate) use session::is_installed;
pub use session::{
    find_session_by_cwd, has_session, kill_session, list_sessions_with_cwd, new_session,
    sanitize_target_part, session_name,
};

use crate::{Error, Result};

pub(super) fn command_unit(output: std::process::Output, context: &'static str) -> Result<()> {
    command_output(output, context).map(|_| ())
}

pub(super) fn command_output(
    output: std::process::Output,
    context: &'static str,
) -> Result<String> {
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }
    Err(Error::Command {
        context,
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}
