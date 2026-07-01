use std::path::Path;

use chrono::Local;

use crate::{git, model::Status, tmux};

mod auto_accept;
mod classify;

use auto_accept::maybe_auto_accept;
use classify::classify_capture;

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
}

pub fn refresh_agent(repo_path: &Path, agent: &mut crate::model::AgentNode, auto_accept: bool) {
    let mut state = WatchStatusState {
        last_capture: agent.last_capture.take(),
        last_change_at: agent.last_change_at.take(),
    };

    if let Some(report) = classify_agent_status(
        repo_path,
        &agent.worktree_path,
        &agent.branch,
        &agent.tmux_session,
        &mut state,
    ) {
        agent.status = report.status;
        // Only a real confirmation prompt drives auto-accept — never a plain
        // input prompt or a finished (`Done`) turn.
        if report.awaiting_confirmation {
            maybe_auto_accept(agent, auto_accept, Local::now());
        }
    }

    agent.last_capture = state.last_capture;
    agent.last_change_at = state.last_change_at;
    agent.shell_working = shell_session_working(&agent.tmux_session);
}

/// Refresh the status of a repo's own main-worktree AI session by *observing*
/// its tmux session — it must never create one, since the main worktree does
/// not auto-launch an AI. When no session exists the status is cleared to
/// `None` so the tree shows no badge; a transient `tmux` probe failure keeps the
/// previous status until the next tick.
pub fn refresh_repo_main(session: &str, repo: &mut crate::model::RepoNode) {
    match tmux::has_session(session) {
        Ok(true) => {}
        Ok(false) => {
            repo.main_status = None;
            repo.main_last_capture = None;
            repo.main_last_change_at = None;
            repo.main_shell_working = false;
            return;
        }
        Err(_) => return,
    }

    let Ok(capture) = tmux::capture_text(session) else {
        return;
    };
    let mut state = WatchStatusState {
        last_capture: repo.main_last_capture.take(),
        last_change_at: repo.main_last_change_at.take(),
    };
    repo.main_status = Some(classify_capture(&capture, &mut state, Local::now()).status);
    repo.main_last_capture = state.last_capture;
    repo.main_last_change_at = state.last_change_at;
    repo.main_shell_working = shell_session_working(session);
}

/// Classify an agent's status. The tmux session is the source of truth for
/// whether the agent process is alive, so it is probed **first**: a live
/// session whose worktree directory still exists reflects its real captured
/// state (`run`/`wait`/`done`/`idle`), while a live session whose worktree was
/// deleted is `Orphaned`. Only once the session is gone does the worktree/branch
/// fall-back distinguish a branch that still exists (`BranchOnly`) from a truly
/// dead agent (`Dead`).
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
            if !worktree_path.exists() {
                return Some(StatusReport {
                    status: Status::Orphaned,
                    awaiting_confirmation: false,
                });
            }
            // A transient capture failure should not corrupt the signal; keep
            // the previous status until the next successful capture.
            let capture = tmux::capture_text(tmux_session).ok()?;
            Some(classify_capture(&capture, state, Local::now()))
        }
        Ok(false) => {
            let status = if !worktree_path.exists()
                && git::branch_exists(repo_path, branch).unwrap_or(false)
            {
                Status::BranchOnly
            } else {
                Status::Dead
            };
            Some(StatusReport {
                status,
                awaiting_confirmation: false,
            })
        }
        Err(_) => None,
    }
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
        Ok(Some(cmd)) => !is_login_shell(&cmd),
        _ => false,
    }
}

/// Whether `cmd` is an interactive shell sitting at its prompt (idle). Login
/// shells report with a leading `-` (e.g. `-zsh`), which is tolerated.
fn is_login_shell(cmd: &str) -> bool {
    matches!(
        cmd.trim_start_matches('-'),
        "zsh" | "bash" | "fish" | "sh" | "dash" | "ksh" | "tcsh" | "csh"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_names_are_idle_other_commands_are_working() {
        assert!(is_login_shell("zsh"));
        assert!(is_login_shell("-zsh"));
        assert!(is_login_shell("bash"));
        assert!(is_login_shell("fish"));
        assert!(!is_login_shell("cargo"));
        assert!(!is_login_shell("robco"));
        assert!(!is_login_shell("sleep"));
        assert!(!is_login_shell("nvim"));
    }
}
