//! Verified worker launches (dropr:554): [`new_worker_session`] creates a
//! session the same way [`session::new_session`] does, but with its safety
//! net — an output tap, and the options that keep both the pane and the
//! server itself from disappearing before that tap can be read — wired in
//! as part of the very same tmux command, then holds the caller until the
//! pane has proven it actually started before the launch is reported as a
//! success.
//!
//! Scoped to worker creation only (`agent::creation::create_agent_with_launch`,
//! the one place every worker-launch path — the TUI `n` key, `robco spawn`,
//! the MCP `robco_agent_create` tool — actually shares) rather than folded
//! into `session::new_session` itself: that function also opens shell tabs
//! and reattaches sessions from the UI thread, where blocking for up to
//! [`LAUNCH_CHECK_ATTEMPTS`] times [`LAUNCH_CHECK_INTERVAL`] on every keypress
//! would trade an invisible bug for a visible one.

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use crate::{Error, Result};

use super::{
    command_unit,
    probe::{PaneProbe, probe_pane},
    session::{self, exact, kill_session},
};

/// Session-identity variables set by whichever AI agent process happens to
/// be running when a new session inherits the tmux server's global
/// environment (dropr:554) — Claude Code's, Codex's, or a robco worker's own.
/// None of them point at anything valid for a freshly launched session, so
/// they are neutralized the same way `ENV_AGENT_ID` / `ENV_PARENT_AGENT_ID`
/// already are: a new worker must never start inside the identity of a
/// session that already died.
pub(super) const INHERITED_IDENTITY_KEYS: [&str; 9] = [
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_PID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDECODE",
    "AI_AGENT",
    "CODEX_COMPANION_SESSION_ID",
    "CODEX_COMPANION_TRANSCRIPT_PATH",
];

/// How many times, and how far apart, a just-launched pane is checked for a
/// crash before the launch is accepted as having actually started
/// (dropr:554). A worker whose program fails at its own start-up — a missing
/// binary, or `getcwd()` failing because the tmux server's own working
/// directory is gone — dies within milliseconds of the fork, well inside this
/// window, without meaningfully slowing down every ordinary launch (or every
/// test that spins up a real session).
const LAUNCH_CHECK_ATTEMPTS: u32 = 8;
const LAUNCH_CHECK_INTERVAL: Duration = Duration::from_millis(100);

/// Where the output tap started by [`verified_new_session_command`] mirrors a
/// session's raw output, for [`read_tapped_output`] to read back. Built from
/// the session name alone (already filesystem-safe — see
/// [`session::sanitize_target_part`]), so no state needs to be threaded from
/// the caller.
fn output_tap_path(session: &str) -> PathBuf {
    std::env::temp_dir().join(format!("robco-launch-{session}.log"))
}

/// Stops the tap [`verified_new_session_command`] started and removes its file.
/// Best-effort either way: a session already killed for having crashed has
/// nothing left to stop piping from, and the file is still removed.
fn stop_output_tap(session: &str, log_path: &Path) {
    let _ = Command::new("tmux")
        .args(["pipe-pane", "-t", &exact(session)])
        .output();
    let _ = std::fs::remove_file(log_path);
}

fn read_tapped_output(log_path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(log_path).ok()?;
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// Quotes one shell word for the `pipe-pane -o` command line, the same way
/// `overseer::discord::ops_session_tmux`'s own log-path quoting does: single
/// quoted, with embedded single quotes escaped.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Builds the `new-session` command for a worker launch, with its whole
/// safety net chained onto the *same* tmux command via `;` rather than
/// issued as separate follow-up commands.
///
/// That chaining is load-bearing, not cosmetic. A program that fails at its
/// own start-up can exit within milliseconds of the fork — fast enough that
/// a *second*, separately dispatched tmux client command (a fresh process
/// spawn plus a server round trip) can lose the race against it. Losing that
/// race used to be silent in two different ways:
///
/// - `set-option -g exit-empty off`: if the pane that just died was the
///   server's only session, tmux's own default (`exit-empty on`) tears the
///   *entire server* down the moment it notices — not just the one session —
///   so every following command in this file, run as a separate client
///   connection, finds no server left to connect to at all. Chaining this in
///   means the option lands before the server can ever observe an empty
///   session count.
/// - `pipe-pane -o 'cat >> <log>'`: attaching a pipe to an *already-exited*
///   process fails outright (`target pane has exited`) even though
///   `remain-on-exit` kept the pane object itself alive. The pipe has to be
///   attached while the process can still be piped from, not after.
///
/// `set-window-option remain-on-exit on` sits between the two for the same
/// reason: the pane must still exist for `pipe-pane` to target it at all.
fn verified_new_session_command(
    session: &str,
    cwd: &Path,
    program: &str,
    envs: &[(&str, String)],
    log_path: &Path,
) -> Command {
    let target = exact(session);
    let pipe_command = format!("cat >> {}", shell_quote(&log_path.display().to_string()));
    let mut command = session::new_session_command(session, cwd, program, envs);
    command.args([
        ";",
        "set-option",
        "-g",
        "exit-empty",
        "off",
        ";",
        "set-window-option",
        "-t",
        &target,
        "remain-on-exit",
        "on",
        ";",
        "pipe-pane",
        "-t",
        &target,
        "-o",
        &pipe_command,
    ]);
    command
}

/// Creates a session the same way [`session::new_session`] does, with its
/// whole safety net wired into that same tmux command (see
/// [`verified_new_session_command`]), then holds the caller until the pane
/// has proven it actually started (dropr:554). See the module docs for why
/// this is a separate entry point rather than a step inside `new_session`
/// itself.
pub fn new_worker_session(
    session: &str,
    cwd: &Path,
    program: &str,
    envs: &[(&str, String)],
) -> Result<()> {
    let log_path = output_tap_path(session);
    let _ = std::fs::remove_file(&log_path);
    let output = verified_new_session_command(session, cwd, program, envs, &log_path).output()?;
    command_unit(output, "tmux new-session")?;
    session::apply_cosmetic_options(session);
    verify_launch(session, cwd, &log_path)
}

/// Confirms a just-created session actually started running, before its
/// launch is reported as a success (dropr:554).
///
/// Two failure modes get caught here, both invisible before this existed
/// because `tmux new-session` itself succeeds the instant the pane is
/// registered, regardless of what the program inside it does next:
///
/// - The program exits right away (a missing binary, or a crash at its own
///   start-up). The output tap and `remain-on-exit`, both already active by
///   the time this runs (see [`verified_new_session_command`]), mean the
///   crash is caught even when it happens before this function's very first
///   probe; the broken session is killed once that is confirmed.
/// - The program is running, but not where it was told to: `-c cwd` was
///   passed to `new-session`, yet the pane's actual working directory
///   disagrees. This is the dropr:554 root cause — a tmux *server* whose own
///   working directory had been deleted handed every new pane that same dead
///   directory, even panes explicitly given `-c`.
///
/// The tap and `remain-on-exit` are both undone once the pane is confirmed
/// alive and correctly placed, so a session that later exits normally still
/// tears itself down the way every other part of robco already assumes.
/// `exit-empty` is deliberately left off: it is a server-wide option, not a
/// per-session one, and turning it back on would just reopen the same race
/// for the next launch this server ever sees.
fn verify_launch(session: &str, cwd: &Path, log_path: &Path) -> Result<()> {
    let expected = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let mut outcome = Ok(());

    for _ in 0..LAUNCH_CHECK_ATTEMPTS {
        match probe_pane(session) {
            PaneProbe::Dead => {
                let detail = read_tapped_output(log_path)
                    .unwrap_or_else(|| "(no output captured)".to_string());
                let _ = kill_session(session);
                outcome = Err(Error::WorkerLaunchCrashed {
                    session: session.to_string(),
                    detail,
                });
                break;
            }
            PaneProbe::Gone => {
                let detail = read_tapped_output(log_path).unwrap_or_else(|| {
                    "session ended before its output could be captured".to_string()
                });
                outcome = Err(Error::WorkerLaunchCrashed {
                    session: session.to_string(),
                    detail,
                });
                break;
            }
            PaneProbe::Alive(actual) => {
                let actual_canon = actual.canonicalize().unwrap_or_else(|_| actual.clone());
                if actual_canon != expected {
                    let _ = kill_session(session);
                    outcome = Err(Error::WorkerLaunchWrongCwd {
                        session: session.to_string(),
                        expected: cwd.to_path_buf(),
                        actual,
                    });
                    break;
                }
            }
        }
        std::thread::sleep(LAUNCH_CHECK_INTERVAL);
    }

    stop_output_tap(session, log_path);
    if outcome.is_ok() {
        let _ = Command::new("tmux")
            .args([
                "set-window-option",
                "-t",
                &exact(session),
                "remain-on-exit",
                "off",
            ])
            .output();
    }
    outcome
}

#[cfg(test)]
#[path = "launch_tests.rs"]
mod tests;
