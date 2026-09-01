use std::path::PathBuf;

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::dropr::{DroprTaskFetch, DroprWorkspace};

use super::{AgentNode, Status};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HostLabel {
    pub name: String,
    pub ssh: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoNode {
    pub path: PathBuf,
    pub name: String,
    pub remote_url: Option<String>,
    /// Persisted manual registration; keeps an agent-less repo listed.
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub agents: Vec<AgentNode>,
    /// Owning remote host. Runtime only; refreshed each tick and never persisted.
    #[serde(skip)]
    pub host: Option<HostLabel>,
    #[serde(skip)]
    pub dropr: Option<DroprWorkspace>,
    /// Result of the last dropr task fetch, failures included: a pane that
    /// cannot tell a failed fetch from an empty board misreports both.
    #[serde(skip)]
    pub dropr_tasks: DroprTaskFetch,
    /// Status of the repo's own main-worktree AI session, or `None` when no such
    /// session is running (the main worktree does not auto-launch one). Runtime
    /// only; refreshed each tick and never persisted.
    #[serde(skip)]
    pub main_status: Option<Status>,
    #[serde(skip)]
    pub main_last_capture: Option<String>,
    /// Last observed spinner frame for main-session motion detection.
    #[serde(skip)]
    pub main_last_spinner: Option<String>,
    #[serde(skip)]
    pub main_last_change_at: Option<DateTime<Local>>,
    /// Whether the repo main-worktree companion shell (TERM) session is running
    /// a foreground command. Runtime only; refreshed each tick, never persisted.
    #[serde(skip)]
    pub main_shell_working: bool,
    /// Whether the main AI session has an in-flight tool call. Runtime only;
    /// refreshed each tick and never persisted.
    #[serde(skip)]
    pub main_mcp_active: bool,
    #[serde(skip)]
    pub main_pane_pid: Option<u32>,
    #[serde(skip)]
    pub main_tracked_command: Option<String>,
    #[serde(skip)]
    pub main_subagents_active: usize,
    /// How many commits the primary checkout's local `main` trails
    /// `origin/main` by, from whatever `origin/main` was last known to be —
    /// no network fetch of its own. `None` when it is not behind, or the
    /// comparison could not be made at all (no local `main`, no `origin`,
    /// not a repository). Runtime only; refreshed each tick, never
    /// persisted.
    #[serde(skip)]
    pub main_behind_origin: Option<u32>,
    /// Where the primary checkout's `HEAD` actually points, probed on the
    /// same slow discovery cadence as `main_behind_origin`, next to it.
    /// `None` when it is on `main` — the safe state — or the probe has not
    /// run yet; `Some` is always something a repo summary warns about.
    /// Runtime only; refreshed each tick, never persisted.
    #[serde(skip)]
    pub checkout_state: Option<CheckoutState>,
}

/// What can be wrong with the primary checkout's `HEAD`: it is detached, it
/// is on a named branch that is not the repository's own default branch, or
/// that default branch itself could not be resolved at all. Every case but
/// the last leaves plain `git pull` failing for the operator — see
/// `RepoNode::checkout_state`. The default branch name carried by the first
/// two variants is resolved once, on the same tick this state itself is
/// computed (`status::refresh_checkout_branch`) — see dropr:503.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckoutState {
    Detached {
        default_branch: String,
    },
    OtherBranch {
        current: String,
        default_branch: String,
    },
    /// `origin/HEAD` could not be read at all — no remote, or a remote
    /// whose `HEAD` was never fetched. Fixed by `git remote set-head origin
    /// -a`, which the operator has to run; robco never guesses a name.
    DefaultBranchUnknown,
}
