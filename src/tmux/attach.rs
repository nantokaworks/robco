use std::process::Command;

use crossterm::terminal;

use crate::{Error, Result};

use super::{
    command_unit,
    keys::{ReturnKeyBinding, set_session_option, wait_for_return_key},
    session::{exact, has_session, resize_session},
};

pub fn attach(session: &str) -> Result<()> {
    let in_tmux = std::env::var_os("TMUX").is_some();
    if !in_tmux {
        let (width, height) = terminal::size()?;
        if width != 0 && height != 0 {
            resize_session(session, width, height)?;
        }
    }
    // Hand window sizing back to the attaching client. The preview path pins
    // `window-size` to `manual`; if we leave it pinned, the window stays at our
    // pre-attach guess (or the small preview size) and does not track the real
    // client. tmux then cannot reserve the row consumed by the status bar that
    // `install()` turns on, so the "C-q to return" line is drawn over the inner
    // program's bottom row (e.g. Claude's mode indicator). Letting the client
    // drive the size (as ClaudeSquad does) makes tmux lay out `pane = client - 1`
    // status row on attach, so nothing overlaps and no filler rows appear.
    let _ = set_session_option(session, "window-size", "latest");

    let binding = ReturnKeyBinding::install(in_tmux, session)?;
    let mut command = Command::new("tmux");
    if in_tmux {
        command.args(["switch-client", "-t", &exact(session)]);
    } else {
        command.args(["attach", "-t", &exact(session)]);
    }
    let status = command.status()?;
    let attach_result = if status.success() {
        if in_tmux {
            wait_for_return_key(session)
        } else {
            Ok(())
        }
    } else {
        Err(Error::Command {
            context: "tmux attach",
            stderr: format!("tmux exited with {status}"),
        })
    };
    let restore_result = binding.restore();
    if !has_session(session)? {
        return restore_result.map(|_| ());
    }
    attach_result.and(restore_result)
}

pub fn send_keys(session: &str, keys: &[&str]) -> Result<()> {
    let output = Command::new("tmux")
        .args(["send-keys", "-t", &exact(session)])
        .args(keys)
        .output()?;
    command_unit(output, "tmux send-keys")
}
