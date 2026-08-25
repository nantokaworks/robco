//! Verified worker launches (dropr:554): [`new_worker_session`] wraps
//! [`session::new_session`] with a check that the pane it just created
//! actually stayed up and landed where it was told, before the launch is
//! reported as a success.
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

use super::session::{self, exact, kill_session};

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

/// What a just-launched pane looked like on one probe.
enum PaneProbe {
    /// Still running, in the given `pane_current_path`.
    Alive(PathBuf),
    /// The launched program already exited; `remain-on-exit` (set by the
    /// caller before probing) kept the pane around so its last screen can
    /// still be read.
    Dead,
    /// The session is gone entirely — the program exited before
    /// `remain-on-exit` could take effect, so tmux tore the pane, and with it
    /// the session, down before anything could be captured.
    Gone,
}

fn probe_pane(session: &str) -> PaneProbe {
    let Ok(output) = Command::new("tmux")
        .args([
            "display-message",
            "-p",
            "-t",
            &exact(session),
            "#{pane_dead}\t#{pane_current_path}",
        ])
        .output()
    else {
        return PaneProbe::Gone;
    };
    if !output.status.success() {
        return PaneProbe::Gone;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    match text.trim_end_matches(['\n', '\r']).split_once('\t') {
        Some(("1", _)) => PaneProbe::Dead,
        // An empty `pane_current_path` alongside a live-looking `pane_dead`
        // reading is the same underlying event, just caught mid-transition:
        // the OS-level cwd lookup `pane_current_path` needs a live process to
        // query, so a pane whose process just exited can report it empty for
        // one probe before `pane_dead` itself catches up. Treat it as dead
        // rather than as a (nonsensical) empty working directory.
        Some((_, "")) => PaneProbe::Dead,
        Some((_, path)) => PaneProbe::Alive(PathBuf::from(path)),
        None => PaneProbe::Gone,
    }
}

/// Where [`start_output_tap`] mirrors a session's raw output for
/// [`read_tapped_output`] to read back. Built from the session name alone
/// (already filesystem-safe — see [`session::sanitize_target_part`]), so no
/// state needs to be threaded from the caller.
fn output_tap_path(session: &str) -> PathBuf {
    std::env::temp_dir().join(format!("robco-launch-{session}.log"))
}

/// Starts mirroring a pane's raw output to a file, independent of anything
/// tmux itself later renders or scrolls away.
///
/// `capture-pane` was tried first and dropped: a dead pane's *visible
/// screen* is mostly blank padding plus tmux's own "Pane is dead (status N,
/// ...)" notice, and how much of the program's actual last output survives
/// in scrollback for `capture-pane -S` to reach depends on the pane's
/// terminal size and the tmux build's own screen-clear behavior — both of
/// which differ enough across platforms that CI caught real output going
/// missing in a way a local run never did. `pipe-pane` instead taps the raw
/// byte stream as it is written, so what lands in the file does not depend
/// on rendering at all.
///
/// Started as the very first thing after the pane exists, before
/// `remain-on-exit` is even set: `pipe-pane`'s own attach only needs the
/// pty to still be open, not the higher-level session bookkeeping
/// `remain-on-exit` depends on, so it wins even part of the race
/// `remain-on-exit` itself can lose against a program that exits within
/// milliseconds of the fork.
fn start_output_tap(session: &str) -> PathBuf {
    let log_path = output_tap_path(session);
    let _ = std::fs::remove_file(&log_path);
    let _ = Command::new("tmux")
        .args([
            "pipe-pane",
            "-t",
            &exact(session),
            "-o",
            &format!("cat >> {}", shell_quote(&log_path.display().to_string())),
        ])
        .output();
    log_path
}

/// Stops the tap started by [`start_output_tap`] and removes its file.
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

/// Creates a session the same way [`session::new_session`] does, then holds
/// the caller until the pane has proven it actually started (dropr:554). See
/// the module docs for why this is a separate entry point rather than a step
/// inside `new_session` itself.
pub fn new_worker_session(
    session: &str,
    cwd: &Path,
    program: &str,
    envs: &[(&str, String)],
) -> Result<()> {
    session::new_session(session, cwd, program, envs)?;
    verify_launch(session, cwd)
}

/// Confirms a just-created session actually started running, before its
/// launch is reported as a success (dropr:554).
///
/// Two failure modes get caught here, both invisible before this existed
/// because `tmux new-session` itself succeeds the instant the pane is
/// registered, regardless of what the program inside it does next:
///
/// - The program exits right away (a missing binary, or a crash at its own
///   start-up). [`start_output_tap`] is running from the first moment the
///   pane exists, so its own output survives even if the pane (and
///   `remain-on-exit`, turned on right after) loses the race against how
///   fast it exits; the broken session is killed once that is confirmed.
/// - The program is running, but not where it was told to: `-c cwd` was
///   passed to `new-session`, yet the pane's actual working directory
///   disagrees. This is the dropr:554 root cause — a tmux *server* whose own
///   working directory had been deleted handed every new pane that same dead
///   directory, even panes explicitly given `-c`.
///
/// The tap and `remain-on-exit` are both undone once the pane is confirmed
/// alive and correctly placed, so a session that later exits normally still
/// tears itself down the way every other part of robco already assumes.
fn verify_launch(session: &str, cwd: &Path) -> Result<()> {
    let log_path = start_output_tap(session);
    let _ = Command::new("tmux")
        .args([
            "set-window-option",
            "-t",
            &exact(session),
            "remain-on-exit",
            "on",
        ])
        .output();

    let expected = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let mut outcome = Ok(());

    for _ in 0..LAUNCH_CHECK_ATTEMPTS {
        match probe_pane(session) {
            PaneProbe::Dead => {
                let detail = read_tapped_output(&log_path)
                    .unwrap_or_else(|| "(no output captured)".to_string());
                let _ = kill_session(session);
                outcome = Err(Error::WorkerLaunchCrashed {
                    session: session.to_string(),
                    detail,
                });
                break;
            }
            PaneProbe::Gone => {
                let detail = read_tapped_output(&log_path).unwrap_or_else(|| {
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

    stop_output_tap(session, &log_path);
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
