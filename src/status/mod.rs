use std::path::Path;

use chrono::Local;

use crate::{git, model::Status, tmux};

mod auto_accept;
mod classify;
pub mod proc;

use auto_accept::maybe_auto_accept;
use classify::classify_capture;
use proc::ProcSnapshot;

#[derive(Debug, Default)]
pub struct WatchStatusState {
    pub last_capture: Option<String>,
    pub last_change_at: Option<chrono::DateTime<Local>>,
}

/// Outcome of classifying a capture: the display [`Status`] plus whether the
/// screen shows a real confirmation prompt (y/n / option list). The latter —
/// not the broader `Waiting`/`Done` — is what gates auto-accept, so an ordinary
/// input prompt never gets a spurious `y`+Enter typed into it.
#[derive(Debug, Clone, Copy)]
pub struct StatusReport {
    pub status: Status,
    pub awaiting_confirmation: bool,
    pub worktree_missing: bool,
}

pub fn refresh_agent(
    repo_path: &Path,
    agent: &mut crate::model::AgentNode,
    auto_accept: bool,
    processes: Option<&ProcSnapshot>,
) {
    let mut state = WatchStatusState {
        last_capture: agent.last_capture.take(),
        last_change_at: agent.last_change_at.take(),
    };

    let report = classify_agent_status(
        repo_path,
        &agent.worktree_path,
        &agent.branch,
        &agent.tmux_session,
        &mut state,
    );
    agent.last_capture = state.last_capture;
    agent.last_change_at = state.last_change_at;
    agent.shell_working = shell_session_working(&agent.tmux_session);
    match report {
        Some(report) => {
            agent.status = report.status;
            agent.worktree_missing = report.worktree_missing;
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
                );
            }
        }
        None => {
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
) {
    repo.main_tracked_command = None;
    match tmux::has_session(session) {
        Ok(true) => {}
        Ok(false) => {
            repo.main_status = None;
            repo.main_last_capture = None;
            repo.main_last_change_at = None;
            repo.main_shell_working = false;
            repo.main_pane_pid = None;
            repo.main_tracked_command = None;
            return;
        }
        Err(_) => {
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
        last_change_at: repo.main_last_change_at.take(),
    };
    let report = classify_capture(&capture, &mut state, Local::now());
    repo.main_status = Some(downgrade_running_shell_pane(report, session).status);
    repo.main_last_capture = state.last_capture;
    repo.main_last_change_at = state.last_change_at;
    repo.main_shell_working = shell_session_working(session);
    refresh_process(
        session,
        &mut repo.main_pane_pid,
        &mut repo.main_tracked_command,
        processes,
    );
}

fn refresh_process(
    session: &str,
    pane_pid: &mut Option<u32>,
    tracked_command: &mut Option<String>,
    processes: Option<&ProcSnapshot>,
) {
    *tracked_command = None;
    let Some(processes) = processes else {
        return;
    };
    if pane_pid.is_none_or(|pid| !processes.contains(pid)) {
        *pane_pid = tmux::pane_pid(session).ok().flatten();
    }
    *tracked_command = pane_pid.and_then(|pid| processes.tracked_command(pid));
}

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
) -> Option<StatusReport> {
    match tmux::has_session(tmux_session) {
        Ok(true) => {
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
            Some(downgrade_running_shell_pane(report, tmux_session))
        }
        Ok(false) => {
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
        Err(_) => None,
    }
}

fn downgrade_running_shell_pane(report: StatusReport, session: &str) -> StatusReport {
    if report.status != Status::Running {
        return report;
    }
    let pane_command = tmux::pane_current_command(session).ok().flatten();
    shell_pane_downgrade(report, pane_command.as_deref())
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
    })
}

/// Whether the companion `<ai>-shell` TERM session is running a foreground
/// command (anything other than the shell itself). Detected via tmux's
/// `#{pane_current_command}` rather than by scraping output. Returns `false`
/// when the shell session does not exist or the probe fails transiently, so a
/// missing/So-far-unopened TERM never shows a spurious "working" mark.
fn shell_session_working(ai_session: &str) -> bool {
    let shell = format!("{ai_session}-shell");
    if !matches!(tmux::has_session(&shell), Ok(true)) {
        return false;
    }
    match tmux::pane_current_command(&shell) {
        Ok(Some(cmd)) => !proc::is_shell_name(&cmd),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn now() -> chrono::DateTime<Local> {
        Local.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap()
    }

    #[test]
    fn live_running_session_reports_missing_worktree() {
        let mut state = WatchStatusState::default();
        let report = classify_agent_observation(
            true,
            Some("Generating response (esc to interrupt)"),
            false,
            false,
            &mut state,
            now(),
        )
        .unwrap();

        assert_eq!(report.status, Status::Running);
        assert!(!report.awaiting_confirmation);
        assert!(report.worktree_missing);
    }

    #[test]
    fn live_confirmation_reports_waiting_and_missing_worktree() {
        let mut state = WatchStatusState::default();
        let report = classify_agent_observation(
            true,
            Some("Allow edit src/main.rs? (y/n)"),
            false,
            false,
            &mut state,
            now(),
        )
        .unwrap();

        assert_eq!(report.status, Status::Waiting);
        assert!(report.awaiting_confirmation);
        assert!(report.worktree_missing);
    }

    #[test]
    fn live_session_clears_missing_flag_when_worktree_reappears() {
        let mut state = WatchStatusState::default();
        let missing = classify_agent_observation(
            true,
            Some("Generating response (esc to interrupt)"),
            false,
            false,
            &mut state,
            now(),
        )
        .unwrap();
        let restored = classify_agent_observation(
            true,
            Some("Generating response (esc to interrupt)"),
            true,
            false,
            &mut state,
            now(),
        )
        .unwrap();

        assert!(missing.worktree_missing);
        assert!(!restored.worktree_missing);
    }

    #[test]
    fn dead_session_paths_keep_missing_flag_clear() {
        let mut state = WatchStatusState::default();
        let branch_only =
            classify_agent_observation(false, None, false, true, &mut state, now()).unwrap();
        let dead =
            classify_agent_observation(false, None, false, false, &mut state, now()).unwrap();

        assert_eq!(branch_only.status, Status::BranchOnly);
        assert!(!branch_only.worktree_missing);
        assert_eq!(dead.status, Status::Dead);
        assert!(!dead.worktree_missing);
    }

    fn report(status: Status) -> StatusReport {
        StatusReport {
            status,
            awaiting_confirmation: true,
            worktree_missing: true,
        }
    }

    #[test]
    fn shell_pane_downgrade_only_changes_running_shells() {
        assert_eq!(
            shell_pane_downgrade(report(Status::Running), Some("zsh")).status,
            Status::Idle
        );
        for command in [Some("claude"), Some("2_1_208"), None] {
            assert_eq!(
                shell_pane_downgrade(report(Status::Running), command).status,
                Status::Running
            );
        }
        for status in [
            Status::Idle,
            Status::Waiting,
            Status::Done,
            Status::Dead,
            Status::BranchOnly,
        ] {
            let unchanged = shell_pane_downgrade(report(status), Some("bash"));
            assert_eq!(unchanged.status, status);
            assert!(unchanged.awaiting_confirmation);
            assert!(unchanged.worktree_missing);
        }
    }

    #[test]
    fn stale_working_marker_in_shell_pane_is_not_running() {
        let classified = classify_capture(
            "Generating response (esc to interrupt)",
            &mut WatchStatusState::default(),
            now(),
        );

        assert_eq!(classified.status, Status::Running);
        assert_eq!(
            shell_pane_downgrade(classified, Some("zsh")).status,
            Status::Idle
        );
    }
}
