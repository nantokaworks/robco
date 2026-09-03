//! The shell guard robco installs into every Claude worker it creates.
//!
//! robco runs every agent session and the operator's own chat on one tmux
//! server. One ordinary shell command from one worker can therefore end all
//! of them at once, and the worker has no way to know that: it does not know
//! it is inside tmux, and it does not know that `TMUX_TMPDIR` fails to
//! isolate a tmux client that runs there. When `TMUX` is set, tmux takes the
//! socket path from `TMUX` and ignores `TMUX_TMPDIR`, so a probe that means
//! to end its own throwaway server ends the real one instead.
//!
//! This is not a mistake a prompt can train away, so the worker's client is
//! given a `PreToolUse` hook (see `crate::agent::hooks`) that calls
//! `robco guard tmux` before every shell command it runs. The hook reads the
//! client's JSON on stdin and answers with a deny decision when the command
//! would reach the shared server.
//!
//! A command that names its own server with `-S <path>` or `-L <label>` is
//! always allowed: it cannot reach the shared one, so an isolated probe stays
//! possible. Single-session kills stay allowed too — robco itself performs
//! them on merge and on request.

use std::{io::Read, process::ExitCode};

use serde_json::json;

use crate::cli::GuardKind;

/// Shell operators that end one command and begin another. Splitting on them
/// keeps `tmux ls; some-other-command kill-server` from reading as a single
/// tmux invocation. Both halves of `&&` and `||` split here as well, and the
/// empty segment between them is simply never blocked.
const SEPARATORS: [char; 7] = [';', '|', '&', '\n', '(', ')', '`'];

/// Runs one guard against the hook payload on stdin, and prints a deny
/// decision when the command is not allowed to run.
///
/// Always exits 0. A hook that fails is a hook the client reports as broken,
/// and an unreadable or unparseable payload tells us nothing about the
/// command — refusing everything at that point would stop the worker rather
/// than protect it, so an unreadable payload allows the command.
pub fn run(kind: GuardKind) -> ExitCode {
    let mut payload = String::new();
    if std::io::stdin().read_to_string(&mut payload).is_err() {
        return ExitCode::SUCCESS;
    }
    let command = serde_json::from_str::<serde_json::Value>(&payload)
        .ok()
        .and_then(|payload| {
            payload
                .get("tool_input")?
                .get("command")?
                .as_str()
                .map(str::to_string)
        })
        .unwrap_or_default();
    match kind {
        GuardKind::Tmux => {
            if ends_a_shared_tmux_server(&command) {
                println!("{}", deny(TMUX_REASON));
            }
        }
    }
    ExitCode::SUCCESS
}

/// Whether `command` could end the shared tmux server, or every session on it.
pub fn ends_a_shared_tmux_server(command: &str) -> bool {
    command.split(SEPARATORS).any(segment_ends_a_server)
}

fn segment_ends_a_server(segment: &str) -> bool {
    let words: Vec<&str> = segment.split_whitespace().map(unquote).collect();
    words.iter().enumerate().any(|(index, word)| {
        let rest = &words[index + 1..];
        match basename(word) {
            "tmux" => tmux_call_ends_a_server(rest),
            // `pkill -f tmux` and `killall tmux` reach the server without
            // naming a socket at all, so the `-S` / `-L` escape hatch below
            // cannot apply to them.
            "pkill" | "killall" => rest.iter().any(|argument| argument.contains("tmux")),
            _ => false,
        }
    })
}

fn tmux_call_ends_a_server(arguments: &[&str]) -> bool {
    // `-S <path>` and `-L <label>` name a server of the caller's own. Such a
    // call cannot reach the shared server, so it is allowed to end whatever
    // it started.
    if arguments
        .iter()
        .any(|argument| *argument == "-S" || *argument == "-L")
    {
        return false;
    }
    if arguments.contains(&"kill-server") {
        return true;
    }
    // `kill-session -a` keeps only the current session and ends every other
    // one, which for robco means every other worker plus the operator's chat.
    arguments.contains(&"kill-session")
        && arguments
            .iter()
            .any(|argument| short_flag_carries(argument, 'a'))
}

/// Whether `argument` is a short flag cluster (`-a`, `-at`) carrying `letter`.
/// A long flag (`--all`) and a bare value are both excluded, so a session
/// named `-ish` in a `-t` target cannot read as a flag.
fn short_flag_carries(argument: &str, letter: char) -> bool {
    argument.starts_with('-') && !argument.starts_with("--") && argument[1..].contains(letter)
}

/// The last path component, so `/opt/homebrew/bin/tmux` reads as `tmux`.
fn basename(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

/// Strips one layer of surrounding shell quotes.
fn unquote(word: &str) -> &str {
    word.trim_matches(|character| character == '\'' || character == '"')
}

fn deny(reason: &str) -> String {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }
    })
    .to_string()
}

const TMUX_REASON: &str = "robco blocked this command: it can end the shared tmux server, or \
every session on it. robco runs every agent session and the operator's chat on that one server, \
so all of them would close at once. TMUX_TMPDIR does not protect you here: inside a tmux session, \
tmux takes the socket path from $TMUX and ignores TMUX_TMPDIR. If you need a throwaway tmux \
server, unset TMUX and name a short socket path of your own, for example `env -u TMUX tmux -S \
/tmp/probe.sock new-session -d ...`; a call that names its own server with -S or -L is allowed to \
end it. A socket path under the agent scratchpad is too long for a UNIX socket, so keep it short. \
To close one session, target it by name: tmux kill-session -t '=<name>'.";

#[cfg(test)]
#[path = "guard_tests.rs"]
mod tests;
