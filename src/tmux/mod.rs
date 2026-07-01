mod attach;
mod capture;
mod keys;
mod session;

pub use attach::{attach, send_keys};
pub use capture::{capture_plain, capture_text, pane_current_command};
#[allow(unused_imports)]
pub use session::{
    has_session, kill_session, new_session, new_session_command, resize_session,
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
