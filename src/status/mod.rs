use chrono::Local;

use crate::model::Status;

mod auto_accept;
mod classify;
mod observe;
pub mod proc;
mod refresh;
mod shell_session;

pub use observe::{classify_agent_status, classify_session_status};
pub use refresh::{refresh_agent, refresh_main_drift, refresh_repo_main};

#[derive(Debug, Default, Clone)]
pub struct WatchStatusState {
    pub last_capture: Option<String>,
    pub last_spinner: Option<String>,
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
    pub mcp_active: bool,
}
