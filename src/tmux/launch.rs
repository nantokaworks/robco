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

use super::{
    history_size,
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

/// Reads a dead pane's own output, not just its final on-screen state.
///
/// A dead pane's visible screen is mostly blank padding plus tmux's own
/// "Pane is dead (status N, ...)" notice pinned at the bottom — the
/// program's actual last output has already scrolled into history by the
/// time this runs. Capturing from the top of that history (`-S -<history>`)
/// forward pulls it back in, so a crash the caller cares about (`ENOENT`, a
/// stack trace, whatever the program printed on its way out) is not silently
/// replaced by tmux's own bookkeeping message.
fn capture_dead_pane_text(session: &str) -> Option<String> {
    let history = history_size(session).unwrap_or(0);
    let output = Command::new("tmux")
        .args([
            "capture-pane",
            "-p",
            "-S",
            &format!("-{history}"),
            "-t",
            &exact(session),
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
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
///   start-up). `remain-on-exit` is turned on first so the dead pane's
///   screen survives long enough to read with [`capture_dead_pane_text`],
///   then the broken session is killed rather than left behind.
/// - The program is running, but not where it was told to: `-c cwd` was
///   passed to `new-session`, yet the pane's actual working directory
///   disagrees. This is the dropr:554 root cause — a tmux *server* whose own
///   working directory had been deleted handed every new pane that same dead
///   directory, even panes explicitly given `-c`.
///
/// `remain-on-exit` is restored to its default (off) once the pane is
/// confirmed alive and correctly placed, so a session that later exits
/// normally still tears itself down the way every other part of robco
/// already assumes.
fn verify_launch(session: &str, cwd: &Path) -> Result<()> {
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

    for _ in 0..LAUNCH_CHECK_ATTEMPTS {
        match probe_pane(session) {
            PaneProbe::Dead => {
                let detail = capture_dead_pane_text(session)
                    .unwrap_or_else(|| "(no output captured)".to_string());
                let _ = kill_session(session);
                return Err(Error::WorkerLaunchCrashed {
                    session: session.to_string(),
                    detail,
                });
            }
            PaneProbe::Gone => {
                return Err(Error::WorkerLaunchCrashed {
                    session: session.to_string(),
                    detail: "session ended before its output could be captured".to_string(),
                });
            }
            PaneProbe::Alive(actual) => {
                let actual_canon = actual.canonicalize().unwrap_or_else(|_| actual.clone());
                if actual_canon != expected {
                    let _ = kill_session(session);
                    return Err(Error::WorkerLaunchWrongCwd {
                        session: session.to_string(),
                        expected: cwd.to_path_buf(),
                        actual,
                    });
                }
            }
        }
        std::thread::sleep(LAUNCH_CHECK_INTERVAL);
    }

    let _ = Command::new("tmux")
        .args([
            "set-window-option",
            "-t",
            &exact(session),
            "remain-on-exit",
            "off",
        ])
        .output();
    Ok(())
}

#[cfg(test)]
#[path = "launch_tests.rs"]
mod tests;
