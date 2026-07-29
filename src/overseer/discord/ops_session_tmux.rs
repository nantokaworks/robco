//! Runs one Discord ops-agent turn inside a named tmux session instead of a
//! bare child process (dropr:371), so a running turn is discoverable and
//! attachable from the TUI. The `result.json` contract `ops_result::is_complete`
//! polls for is unchanged; only how the CLI is launched and how its liveness
//! is detected differ from `EphemeralSession::run_controlled`.
//!
//! The session is torn down at the end of the turn: the launched command is
//! the pane's only process, so tmux destroys the session itself once it
//! exits, and [`run_in_tmux`] force-kills it on cancellation or timeout. A
//! channel therefore has a live, attachable session only while a turn is
//! actually running — never idle between turns.

use crate::{
    agent::env::launch_command,
    config::Profile,
    overseer::{
        discord::ops_result,
        session::{
            EphemeralSession, SessionControl, SessionResult, env::SessionEnv, resolve_program_impl,
        },
    },
    tmux,
};
use std::{
    fs,
    path::Path,
    thread,
    time::{Duration, Instant},
};

/// How often the poll loop checks `result.json` and the session's liveness.
/// Coarser than `EphemeralSession`'s 25ms: each tick here spawns a
/// `tmux has-session` subprocess, which a bare `Child::try_wait` does not need.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

#[allow(clippy::too_many_arguments)]
pub(super) fn run_in_tmux(
    case_dir: &Path,
    profile: &Profile,
    timeout: Duration,
    env: &SessionEnv,
    prompt: &str,
    session_name: &str,
    control: &SessionControl,
) -> SessionResult {
    let result_path = case_dir.join("result.json");
    let _ = fs::remove_file(&result_path);
    if control.is_cancelled() {
        return SessionResult::LaunchFailed("session cancelled".into());
    }
    let Some(program) = resolve_program_impl(&profile.program) else {
        return SessionResult::LaunchFailed(format!(
            "triage program not found on PATH: {} (configure profile.program with an absolute path)",
            profile.program
        ));
    };
    let log_path = case_dir.join("session.log");
    let mut args = profile.autonomous_args.clone();
    if let Some(model) = &profile.model {
        args.push("--model".into());
        args.push(model.clone());
    }
    let command = format!(
        "{} 2>>{}",
        launch_command(&program.to_string_lossy(), Some(prompt), &args),
        shell_quote(&log_path.display().to_string())
    );
    let pairs = env.pairs();
    let envs = pairs
        .iter()
        .map(|(name, value)| (name.as_str(), value.clone()))
        .collect::<Vec<_>>();
    if let Err(error) = tmux::new_session(session_name, case_dir, &command, &envs) {
        return SessionResult::LaunchFailed(error.to_string());
    }
    control.set_pid(tmux::pane_pid(session_name).ok().flatten());
    let result = poll_tmux(&result_path, session_name, timeout, control);
    let _ = tmux::kill_session(session_name);
    control.set_pid(None);
    EphemeralSession {
        profile,
        case_dir,
        timeout,
        env,
        prompt,
    }
    .classify(result, &log_path)
}

fn poll_tmux(
    result_path: &Path,
    session_name: &str,
    timeout: Duration,
    control: &SessionControl,
) -> SessionResult {
    let deadline = Instant::now() + timeout;
    let mut incomplete = None;
    loop {
        if control.is_cancelled() {
            let _ = tmux::kill_session(session_name);
            return SessionResult::LaunchFailed("session cancelled".into());
        }
        if let Ok(raw) = fs::read(result_path) {
            if ops_result::is_complete(&raw) {
                return SessionResult::Result(raw);
            }
            incomplete = Some(raw);
        }
        // A dead session is the tmux-backed equivalent of `Child::try_wait`
        // reporting the process exited: the launched command was the pane's
        // only process, so its exit (or an unreachable tmux server) already
        // ended the session.
        if !tmux::has_session(session_name).unwrap_or(false) {
            return incomplete.map_or(SessionResult::Missing, SessionResult::Result);
        }
        if Instant::now() >= deadline {
            let _ = tmux::kill_session(session_name);
            return incomplete.map_or(SessionResult::TimedOut, SessionResult::Result);
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// Quote one shell word the same way `crate::agent::env`'s launcher quotes
/// its own extra args: single-quoted, with embedded single quotes escaped.
/// Only the log path needs it here — `launch_command` already quotes the
/// program's own args and prompt.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
