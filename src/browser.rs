//! Handing a URL to the operator (dropr:499, dropr:511).
//!
//! There are two very different situations here, and the split is decided at
//! run time, not at compile time.
//!
//! **robco runs where the operator sits.** A launcher process gets the URL.
//! macOS ships `open` with the base OS. Linux has no single equivalent, but
//! every major desktop environment ships `xdg-open` (part of `xdg-utils`),
//! which is the closest thing to a standard opener there. There is no one
//! command that exists on both platforms, so the command name is chosen per
//! target OS at compile time — robco's own CI runs both (`ubuntu-latest` and
//! `macos-latest`), so this file has to compile clean on each.
//!
//! The launcher process (`open` / `xdg-open`) hands the URL to a running
//! browser and exits fast; it does not wait for the browser itself to close.
//! `open` spawns it without waiting, so a slow or unusual `xdg-open` build
//! that *does* block until the browser exits cannot freeze the TUI. The
//! spawned child is reaped on a background thread instead of left as a
//! zombie for the rest of the session.
//!
//! **robco runs over SSH.** The launcher would start a browser on the
//! *server*, where nobody is looking. On a headless box it fails; on a box
//! with a desktop session it is worse, because a real window opens on a
//! screen the operator cannot see. So the URL goes to the terminal instead:
//! the terminal emulator runs on the operator's machine, so an escape
//! sequence written to it is handled there. OSC 52 asks the terminal to put
//! the URL on *its* clipboard, which is the operator's clipboard. `notify.rs`
//! already relies on the same property for OSC 777 desktop notifications, and
//! that works over SSH for exactly this reason.
//!
//! robco does not try to drive a browser on the client through the SSH
//! channel. That needs a reverse tunnel or X forwarding, which robco cannot
//! set up on its own. Operators who *do* wire one up have the `BROWSER`
//! escape hatch below.
//!
//! The URL is also shown as plain text, which every terminal can select and
//! copy. `ui::hyperlink` marks that text up as an OSC 8 hyperlink after each
//! frame, so a terminal with OSC 8 support can open it with a click; that
//! part has to live next to the drawing code, because OSC 8 marks up text in
//! the terminal grid and the grid belongs to ratatui (dropr:512).

use std::{
    env,
    fs::OpenOptions,
    io::{self, Write},
    process::Command,
    thread,
};

use crate::notify;

#[cfg(target_os = "macos")]
const OPEN_COMMAND: &str = "open";

#[cfg(not(target_os = "macos"))]
const OPEN_COMMAND: &str = "xdg-open";

/// The variables an SSH server sets in the session it starts. `SSH_TTY` alone
/// would answer the robco case, since robco needs an interactive session, but
/// some setups strip one of the three, so any of them counts.
const SSH_SESSION_VARS: [&str; 3] = ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"];

/// How the URL reached the operator. The caller says different things for
/// each, because "opened in the browser" is a lie on the SSH route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opened {
    /// A launcher command started on this machine.
    Launcher,
    /// The URL went to the terminal's clipboard instead.
    Clipboard,
}

/// Hands `url` to the operator by whichever route fits this session. On the
/// launcher route this only reports whether the launcher process itself
/// started — once it has, this stops watching it, so a browser that opens to
/// an error page still counts as a successful launch here. The realistic
/// failure this catches is the launcher command missing entirely (no
/// `xdg-open` on a minimal Linux install, say).
pub fn open(url: &str) -> Result<Opened, String> {
    match route(read_env) {
        Route::Launcher(command) => open_with(&command, url).map(|()| Opened::Launcher),
        Route::Clipboard => {
            write_to_terminal(&osc52(url), inside_tmux(read_env)).map(|()| Opened::Clipboard)
        }
    }
}

/// The route [`open`] takes for one URL.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Route {
    /// Spawn this command with the URL as its only argument.
    Launcher(String),
    /// Send the URL to the terminal, because a local browser would open where
    /// the operator cannot see it.
    Clipboard,
}

fn read_env(name: &str) -> Option<String> {
    env::var(name).ok()
}

/// Picks the route from the environment. `read` is a parameter so tests can
/// pin a session without touching the real environment, which edition 2024
/// makes `unsafe` to write to anyway.
///
/// `BROWSER` wins over everything, including the SSH check. It is the usual
/// Unix convention, and it is the escape hatch for an operator who did set up
/// their own opener — say a helper on the client reached through `ssh -R`.
/// robco reads it as one plain command name, not as the colon-separated list
/// with `%s` slots some tools accept; a value in that older form fails to
/// start, and the error names the command so the operator can see why.
fn route<F>(read: F) -> Route
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(command) = read("BROWSER")
        .map(|command| command.trim().to_string())
        .filter(|command| !command.is_empty())
    {
        return Route::Launcher(command);
    }
    if SSH_SESSION_VARS
        .iter()
        .any(|name| read(name).is_some_and(|value| !value.trim().is_empty()))
    {
        return Route::Clipboard;
    }
    Route::Launcher(OPEN_COMMAND.to_string())
}

fn open_with(command: &str, url: &str) -> Result<(), String> {
    let mut child = Command::new(command)
        .arg(url)
        .spawn()
        .map_err(|err| format!("{command} failed to start: {err}"))?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// OSC 52 "set selection", for the `c` (system clipboard) selection. The body
/// is base64, which is what the sequence is defined to carry.
fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64(text.as_bytes()))
}

/// True when this process runs inside a tmux pane. tmux sets `TMUX` for every
/// process it starts, and nothing else does.
fn inside_tmux<F>(read: F) -> bool
where
    F: Fn(&str) -> Option<String>,
{
    read("TMUX").is_some_and(|value| !value.trim().is_empty())
}

/// Sends `payload` to the terminal the operator is looking at.
///
/// Outside tmux this process's stdout *is* the terminal, so it is the right
/// target — and the only correct one, since robco may run beside tmux
/// sessions the operator attached from some other window.
///
/// Inside tmux, stdout only reaches tmux, which forwards OSC 52 on only when
/// the operator turned `set-clipboard` on. Writing to the attached client's
/// own tty skips that question, and it is the route `notify.rs` already takes
/// for OSC 777. Every attached client gets the write, not just the first one
/// that accepts it: `tmux list-clients` answers for the whole server, so the
/// window the operator is actually looking at may not be the first in the
/// list. With no client attached there is nothing better than stdout, so fall
/// back to it.
fn write_to_terminal(payload: &str, inside_tmux: bool) -> Result<(), String> {
    if inside_tmux {
        let delivered = notify::attached_client_ttys()
            .iter()
            .filter(|tty| write_to_tty(tty, payload))
            .count();
        if delivered > 0 {
            return Ok(());
        }
    }
    let mut out = io::stdout();
    out.write_all(payload.as_bytes())
        .and_then(|()| out.flush())
        .map_err(|err| format!("could not write to the terminal: {err}"))
}

fn write_to_tty(tty: &str, payload: &str) -> bool {
    let Ok(mut file) = OpenOptions::new().write(true).open(tty) else {
        return false;
    };
    file.write_all(payload.as_bytes()).is_ok() && file.flush().is_ok()
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding. robco has no base64 dependency, and this is
/// the only place that needs one, so a whole crate would cost more than the
/// handful of lines it replaces.
fn base64(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut triple = 0u32;
        for slot in 0..3 {
            triple = (triple << 8) | u32::from(chunk.get(slot).copied().unwrap_or(0));
        }
        for slot in 0..4 {
            if slot <= chunk.len() {
                let index = (triple >> (18 - slot * 6)) & 0b11_1111;
                encoded.push(BASE64_ALPHABET[index as usize] as char);
            } else {
                encoded.push('=');
            }
        }
    }
    encoded
}

#[cfg(test)]
#[path = "browser_tests.rs"]
mod tests;
