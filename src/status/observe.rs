use std::path::Path;

use chrono::Local;

use crate::{git, model::Status, tmux};

use super::classify::classify_capture;
use super::proc;
use super::session_alive;
use super::{StatusReport, WatchStatusState};

/// Classify an agent's status. The tmux session is the source of truth for
/// whether the agent process is alive, so it is probed **first**. Live sessions
/// always reflect their real captured state
/// (`run`/`wait`/`done`/`idle`). A missing worktree is reported independently.
/// Only once the session is gone does the worktree/branch fall-back distinguish
/// a branch that still exists (`BranchOnly`) from a truly dead agent (`Dead`).
///
/// A transient failure to probe `tmux` (a fork/exec hiccup under load) returns
/// `None`, so the caller keeps the previous status and retries next tick instead
/// of flipping a healthy agent to `Dead`.
pub fn classify_agent_status(
    repo_path: &Path,
    worktree_path: &Path,
    branch: &str,
    tmux_session: &str,
    state: &mut WatchStatusState,
    panes: Option<&tmux::PaneSnapshot>,
) -> Option<StatusReport> {
    match session_alive(panes, tmux_session) {
        Some(true) => {
            // A transient capture failure should not corrupt the signal; keep
            // the previous status until the next successful capture.
            let capture = tmux::capture_text(tmux_session).ok()?;
            let report = classify_agent_observation(
                true,
                Some(&capture),
                worktree_path.exists(),
                false,
                state,
                Local::now(),
            )?;
            Some(downgrade_running_shell_pane(report, tmux_session, panes))
        }
        Some(false) => {
            let worktree_exists = worktree_path.exists();
            let branch_exists =
                !worktree_exists && git::branch_exists(repo_path, branch).unwrap_or(false);
            classify_agent_observation(
                false,
                None,
                worktree_exists,
                branch_exists,
                state,
                Local::now(),
            )
        }
        None => None,
    }
}

/// Classify a bare tmux session — the Overseer's own control session, which
/// has no worktree or branch to fall back on — the same way a session-backed
/// agent or the repo-main row is classified: via [`classify_capture`], not an
/// existence probe, so the row can tell an idle prompt from actual work.
///
/// An absent session reports `None` (no glyph), not `Dead` — there is no
/// worktree/branch history to distinguish from. A transient `tmux` failure
/// (the existence probe or the capture itself) returns `previous` unchanged,
/// the same "keep the last known status" contract [`super::refresh_repo_main`]
/// gives a probe hiccup rather than flipping the row.
pub fn classify_session_status(
    session: &str,
    previous: Option<Status>,
    state: &mut WatchStatusState,
) -> Option<Status> {
    match tmux::has_session(session) {
        Ok(true) => {
            let Ok(capture) = tmux::capture_text(session) else {
                return previous;
            };
            classify_session_observation(Some(&capture), state, Local::now())
        }
        Ok(false) => classify_session_observation(None, state, Local::now()),
        Err(_) => previous,
    }
}

fn classify_session_observation(
    capture: Option<&str>,
    state: &mut WatchStatusState,
    now: chrono::DateTime<Local>,
) -> Option<Status> {
    Some(classify_capture(capture?, state, now).status)
}

pub(super) fn downgrade_running_shell_pane(
    report: StatusReport,
    session: &str,
    panes: Option<&tmux::PaneSnapshot>,
) -> StatusReport {
    if report.status != Status::Running {
        return report;
    }
    let pane_command = panes.and_then(|panes| panes.pane_current_command(session));
    shell_pane_downgrade(report, pane_command)
}

fn shell_pane_downgrade(mut report: StatusReport, pane_command: Option<&str>) -> StatusReport {
    if report.status == Status::Running && pane_command.is_some_and(proc::is_shell_name) {
        report.status = Status::Idle;
    }
    report
}

fn classify_agent_observation(
    session_alive: bool,
    capture: Option<&str>,
    worktree_exists: bool,
    branch_exists: bool,
    state: &mut WatchStatusState,
    now: chrono::DateTime<Local>,
) -> Option<StatusReport> {
    if session_alive {
        let mut report = classify_capture(capture?, state, now);
        report.worktree_missing = !worktree_exists;
        return Some(report);
    }

    Some(StatusReport {
        status: if !worktree_exists && branch_exists {
            Status::BranchOnly
        } else {
            Status::Dead
        },
        awaiting_confirmation: false,
        worktree_missing: false,
        mcp_active: false,
    })
}

#[cfg(test)]
mod tests;
