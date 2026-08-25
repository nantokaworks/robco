mod adoption;
mod creation;
pub(crate) mod dropr_task;
pub mod env;
mod hooks;
mod naming;
mod session;
#[cfg(test)]
pub(crate) mod test_support;

pub use adoption::{adopt_worktree, normalize_adopted_titles};
pub use creation::create_agent;
pub(crate) use creation::{create_agent_with_launch, tui_launch_env};
pub use env::RecoveredIdentity;
pub(crate) use naming::worker_branch_name;
pub use session::{
    ensure_agent_session, ensure_repo_claude_session, ensure_repo_shell_session,
    ensure_shell_session, kill_agent, repo_claude_session_name, repo_shell_session_name,
    restart_agent, shell_session_name,
};
