use std::path::PathBuf;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("toml error: {0}")]
    Toml(#[from] toml_edit::TomlError),
    #[error("home directory could not be resolved")]
    HomeDir,
    #[error("setup wizard: {0}")]
    Wizard(String),
    #[error("{context} failed: {stderr}")]
    Command {
        context: &'static str,
        stderr: String,
    },
    #[error("worktree has tracked changes: {0}")]
    DirtyWorktree(PathBuf),
    #[error(
        "{0}: the repository's default branch could not be resolved; run `git remote set-head origin -a`"
    )]
    DefaultBranchUnresolved(PathBuf),
    #[error("child worktrees remain under {0}; remove them first")]
    ChildWorktreesPresent(PathBuf),
    #[error("robco new must run inside a robco agent session (ROBCO_AGENT_ID is not set)")]
    NewOutsideAgentSession,
    #[error("parent robco agent not found in registry: {0}")]
    ParentAgentNotFound(String),
    #[error("registered repository not found: {0}")]
    RepoSelectorNotFound(String),
    #[error("repository name is ambiguous; use an absolute path: {0}")]
    RepoSelectorAmbiguous(String),
    #[error(
        "child worktree {worktree_path} and tmux session {tmux_session} were created, but the \
         repository disappeared from the registry; the TUI will adopt the child"
    )]
    CreatedChildRepoMissing {
        worktree_path: PathBuf,
        tmux_session: String,
    },
    #[error(
        "--dropr-task cannot be combined with an explicit title, prompt, or name-slug; the task supplies all three"
    )]
    DroprTaskSpawnConflict,
    #[error("no dropr workspace found for repo: {0}")]
    DroprTaskNoWorkspace(String),
    #[error("dropr task not found: {0}")]
    DroprTaskNotFound(String),
    #[error(
        "dropr task {task_ref} is already claimed by {}",
        holder.as_deref().unwrap_or("another agent")
    )]
    DroprTaskClaimed {
        task_ref: String,
        holder: Option<String>,
    },
    #[error("could not claim dropr task {task_ref}: {reason}")]
    DroprTaskClaimRefused { task_ref: String, reason: String },
    #[error("could not reach dropr")]
    DroprUnavailable,
    #[error("could not confirm {0}'s subtasks; try again")]
    DroprSubtasksUnconfirmed(String),
    #[error("worker session {session} exited right after launch: {detail}")]
    WorkerLaunchCrashed { session: String, detail: String },
    #[error("no open pull request for {0}")]
    NoOpenPullRequest(String),
    #[error(
        "'{dir}' is not a directory\nto connect to a remote robco, use --host <destination> or the H key inside the TUI",
        dir = .0.display()
    )]
    LaunchDirMissing(PathBuf),
    #[error(
        "worker session {session} started in {actual}, not {expected}; the tmux server's own \
         working directory is gone and the server needs a restart"
    )]
    WorkerLaunchWrongCwd {
        session: String,
        expected: PathBuf,
        actual: PathBuf,
    },
}
