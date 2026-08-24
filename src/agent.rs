mod creation;
pub(crate) mod dropr_task;
pub mod env;
mod hooks;
mod naming;
mod session;
#[cfg(test)]
mod test_support;

pub(crate) use creation::create_agent_with_launch;
pub use creation::{adopt_worktree, create_agent, normalize_adopted_titles};
pub use env::RecoveredIdentity;
pub(crate) use naming::worker_branch_name;
pub use session::{
    ensure_agent_session, ensure_repo_claude_session, ensure_repo_shell_session,
    ensure_shell_session, kill_agent, repo_claude_session_name, repo_shell_session_name,
    restart_agent, shell_session_name,
};
