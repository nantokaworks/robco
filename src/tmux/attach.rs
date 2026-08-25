use std::io;

use crossterm::{
    cursor::{MoveToColumn, MoveUp},
    execute,
    terminal::{self, Clear, ClearType},
};

use crate::{Error, Result};

use super::{
    TmuxServer, command_unit,
    keys::{ReturnKeyBinding, wait_for_return_key},
    resize::resize_session,
    session::{exact, has_session},
    session_option::set_session_option,
};

pub fn attach(server: &TmuxServer, session: &str) -> Result<()> {
    let in_tmux = std::env::var_os("TMUX").is_some();
    if !in_tmux {
        let (width, height) = terminal::size()?;
        if width != 0 && height != 0 {
            resize_session(server, session, width, height)?;
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
    let _ = set_session_option(server, session, "window-size", "latest");

    let binding = ReturnKeyBinding::install(server, in_tmux, session)?;
    let mut command = server.command();
    if in_tmux {
        command.args(["switch-client", "-t", &exact(session)]);
    } else {
        command.args(["attach", "-t", &exact(session)]);
    }
    let status = command.status()?;
    if !in_tmux {
        // The exiting tmux client prints "[detached (from session …)]" or
        // "[exited]" onto the normal screen buffer; the TUI re-enters the
        // alternate screen right away, so each line piles up unseen and the
        // whole backlog floods the terminal when robco quits. Erase the line
        // (cursor sits one row below it) before handing the terminal back.
        let _ = execute!(
            io::stdout(),
            MoveUp(1),
            Clear(ClearType::CurrentLine),
            MoveToColumn(0)
        );
    }
    let attach_result = if status.success() {
        if in_tmux {
            wait_for_return_key(server, session)
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
    if !has_session(server, session)? {
        return restore_result.map(|_| ());
    }
    attach_result.and(restore_result)
}

pub fn send_keys(server: &TmuxServer, session: &str, keys: &[&str]) -> Result<()> {
    let output = server
        .command()
        .args(["send-keys", "-t", &exact(session)])
        .args(keys)
        .output()?;
    command_unit(output, "tmux send-keys")
}

pub fn send_literal_text(server: &TmuxServer, session: &str, text: &str) -> Result<()> {
    let target = format!("={session}:");
    let output = server
        .command()
        .args(["send-keys", "-t", &target, "-l", "--", text])
        .output()?;
    command_unit(output, "tmux send literal text")
}

/// Collapses text to one line before it is typed into a session.
///
/// A literal send delivers a newline as a submit, so multi-line text would enter
/// the agent's prompt one fragment at a time and be acted on a line at a time.
/// Prompts are authored as prose for readability and flattened here.
pub fn single_line(text: &str) -> String {
    let mut flattened = String::with_capacity(text.len());
    let mut previous_was_control = false;
    for character in text.chars() {
        if character.is_control() {
            if !previous_was_control {
                flattened.push(' ');
            }
            previous_was_control = true;
        } else {
            flattened.push(character);
            previous_was_control = false;
        }
    }
    flattened.trim().to_owned()
}
