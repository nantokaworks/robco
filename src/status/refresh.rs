use std::path::Path;

use chrono::Local;

use crate::{git, model::Status, tmux};

use super::WatchStatusState;
use super::auto_accept::maybe_auto_accept;
use super::classify::classify_capture;
use super::observe::{classify_agent_status, downgrade_running_shell_pane};
use super::proc::ProcSnapshot;
use super::session_alive;
use super::shell_session::shell_session_working;

pub fn refresh_agent(
    repo_path: &Path,
    agent: &mut crate::model::AgentNode,
    auto_accept: bool,
    processes: Option<&ProcSnapshot>,
    panes: Option<&tmux::PaneSnapshot>,
) {
    let mut state = WatchStatusState {
        last_capture: agent.last_capture.take(),
        last_spinner: agent.last_spinner.take(),
        last_change_at: agent.last_change_at.take(),
    };

    let report = classify_agent_status(
        repo_path,
        &agent.worktree_path,
        &agent.branch,
        &agent.tmux_session,
        &mut state,
        panes,
    );
    agent.last_capture = state.last_capture;
    agent.last_spinner = state.last_spinner;
    agent.last_change_at = state.last_change_at;
    agent.shell_working = shell_session_working(&agent.tmux_session, panes);
    match report {
        Some(report) => {
            agent.status = report.status;
            agent.worktree_missing = report.worktree_missing;
            agent.mcp_active = report.mcp_active;
            // Only a real confirmation prompt drives auto-accept — never a
            // plain input prompt or a finished (`Done`) turn.
            if report.awaiting_confirmation {
                maybe_auto_accept(agent, auto_accept, Local::now());
            }
            if matches!(report.status, Status::Dead | Status::BranchOnly) {
                agent.pane_pid = None;
                agent.tracked_command = None;
            } else {
                refresh_process(
                    &agent.tmux_session,
                    &mut agent.pane_pid,
                    &mut agent.tracked_command,
                    processes,
                    panes,
                );
            }
        }
        None => {
            agent.mcp_active = false;
            agent.pane_pid = None;
            agent.tracked_command = None;
        }
    }
}

/// Refresh the status of a repo's own main-worktree AI session by *observing*
/// its tmux session — it must never create one, since the main worktree does
/// not auto-launch an AI. When no session exists the status is cleared to
/// `None` so the tree shows no badge; a transient `tmux` probe failure keeps the
/// previous status until the next tick.
pub fn refresh_repo_main(
    session: &str,
    repo: &mut crate::model::RepoNode,
    processes: Option<&ProcSnapshot>,
    panes: Option<&tmux::PaneSnapshot>,
) {
    repo.main_tracked_command = None;
    match session_alive(panes, session) {
        Some(true) => {}
        Some(false) => {
            repo.main_status = None;
            repo.main_last_capture = None;
            repo.main_last_spinner = None;
            repo.main_last_change_at = None;
            repo.main_shell_working = false;
            repo.main_mcp_active = false;
            repo.main_pane_pid = None;
            repo.main_tracked_command = None;
            return;
        }
        None => {
            repo.main_pane_pid = None;
            return;
        }
    }

    let Ok(capture) = tmux::capture_text(session) else {
        repo.main_pane_pid = None;
        return;
    };
    let mut state = WatchStatusState {
        last_capture: repo.main_last_capture.take(),
        last_spinner: repo.main_last_spinner.take(),
        last_change_at: repo.main_last_change_at.take(),
    };
    let report = classify_capture(&capture, &mut state, Local::now());
    repo.main_mcp_active = report.mcp_active;
    repo.main_status = Some(downgrade_running_shell_pane(report, session, panes).status);
    repo.main_last_capture = state.last_capture;
    repo.main_last_spinner = state.last_spinner;
    repo.main_last_change_at = state.last_change_at;
    repo.main_shell_working = shell_session_working(session, panes);
    refresh_process(
        session,
        &mut repo.main_pane_pid,
        &mut repo.main_tracked_command,
        processes,
        panes,
    );
}

/// Refreshes how far the primary checkout's local default branch trails
/// `origin/<default>`, purely from whichever refs are already known — no
/// fetch of its own. The value only moves when the refs move (a fetch,
/// merge, pull, or push), so this runs on the slower discovery tick rather
/// than the 750 ms status tick. `None` when the default branch is not
/// behind, or the comparison could not be made at all (the repository's own
/// default branch could not be resolved, no local copy of it, no `origin`,
/// not a repository). See dropr:503 — this never assumes `main`.
pub fn refresh_main_drift(repo: &mut crate::model::RepoNode) {
    repo.main_behind_origin = git::default_branch(&repo.path)
        .ok()
        .flatten()
        .and_then(|default_branch| {
            let origin_ref = format!("origin/{default_branch}");
            git::ahead_behind(&repo.path, &default_branch, &origin_ref).ok()
        })
        .map(|(_ahead, behind)| behind)
        .filter(|behind| *behind > 0);
}

/// Refreshes where the primary checkout's `HEAD` actually points, and what
/// the repository's own default branch is — both purely from local reads
/// (`git symbolic-ref`) — no fetch, no network. The value only moves when an
/// operator (or the `c` repair action) moves `HEAD` by hand, or the
/// repository's `origin/HEAD` changes, so this runs on the slower discovery
/// tick rather than the 750 ms status tick, right alongside
/// [`refresh_main_drift`]. `None` when `HEAD` is on the default branch, or a
/// probe failed outright (not a repository, `git` missing) — see
/// `crate::model::CheckoutState::DefaultBranchUnknown` for the case where
/// `HEAD` reads fine but the default branch itself cannot be resolved
/// (dropr:503).
pub fn refresh_checkout_branch(repo: &mut crate::model::RepoNode) {
    use crate::model::CheckoutState;
    let Ok(default_branch) = git::default_branch(&repo.path) else {
        repo.checkout_state = None;
        return;
    };
    let Some(default_branch) = default_branch else {
        repo.checkout_state = Some(CheckoutState::DefaultBranchUnknown);
        return;
    };
    repo.checkout_state = match git::current_branch(&repo.path) {
        Ok(Some(branch)) if branch == default_branch => None,
        Ok(Some(branch)) => Some(CheckoutState::OtherBranch {
            current: branch,
            default_branch,
        }),
        Ok(None) => Some(CheckoutState::Detached { default_branch }),
        Err(_) => None,
    };
}

fn refresh_process(
    session: &str,
    pane_pid: &mut Option<u32>,
    tracked_command: &mut Option<String>,
    processes: Option<&ProcSnapshot>,
    panes: Option<&tmux::PaneSnapshot>,
) {
    *tracked_command = None;
    let Some(processes) = processes else {
        return;
    };
    if pane_pid.is_none_or(|pid| !processes.contains(pid)) {
        *pane_pid = panes.and_then(|panes| panes.pane_pid(session));
    }
    *tracked_command = pane_pid.and_then(|pid| processes.tracked_command(pid));
}

#[cfg(test)]
#[path = "refresh_tests.rs"]
mod tests;
