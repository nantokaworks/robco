use super::env::{agent_env, launch_command, session_id_args};
use crate::{
    Result,
    config::Config,
    git,
    model::{AgentNode, RepoNode},
    tmux,
};

pub fn restart_agent(agent: &AgentNode) -> Result<()> {
    let server = tmux::TmuxServer::default_server();
    let _ = tmux::kill_session(&server, &agent.tmux_session);
    let command = relaunch_command(agent);
    tmux::new_session(
        &server,
        &agent.tmux_session,
        &agent.worktree_path,
        &command,
        &agent_env(&agent.id, agent.parent_agent_id.as_deref()),
    )
}

pub fn ensure_agent_session(agent: &AgentNode) -> Result<()> {
    let server = tmux::TmuxServer::default_server();
    if tmux::has_session(&server, &agent.tmux_session)? {
        return Ok(());
    }

    let command = relaunch_command(agent);
    tmux::new_session(
        &server,
        &agent.tmux_session,
        &agent.worktree_path,
        &command,
        &agent_env(&agent.id, agent.parent_agent_id.as_deref()),
    )
}

fn relaunch_command(agent: &AgentNode) -> String {
    launch_command(
        &agent.program,
        None,
        &session_id_args(agent.claude_session_id.as_deref()),
    )
}

pub fn shell_session_name(agent: &AgentNode) -> String {
    format!("{}-shell", agent.tmux_session)
}

pub fn ensure_shell_session(agent: &AgentNode) -> Result<()> {
    let server = tmux::TmuxServer::default_server();
    let session = shell_session_name(agent);
    if tmux::has_session(&server, &session)? {
        return Ok(());
    }

    tmux::new_session(
        &server,
        &session,
        &agent.worktree_path,
        &shell_program(),
        &[],
    )
}

pub fn repo_shell_session_name(prefix: &str, repo: &RepoNode) -> String {
    format!("{}-shell", tmux::session_name(prefix, &repo.name, "main"))
}

pub fn repo_claude_session_name(prefix: &str, repo: &RepoNode) -> String {
    tmux::session_name(prefix, &repo.name, "main")
}

pub fn ensure_repo_shell_session(prefix: &str, repo: &RepoNode) -> Result<()> {
    let server = tmux::TmuxServer::default_server();
    let session = repo_shell_session_name(prefix, repo);
    if tmux::has_session(&server, &session)? {
        return Ok(());
    }

    tmux::new_session(&server, &session, &repo.path, &shell_program(), &[])
}

pub fn ensure_repo_claude_session(config: &Config, prefix: &str, repo: &RepoNode) -> Result<()> {
    let server = tmux::TmuxServer::default_server();
    let session = repo_claude_session_name(prefix, repo);
    if tmux::has_session(&server, &session)? {
        return Ok(());
    }

    tmux::new_session(
        &server,
        &session,
        &repo.path,
        &repo_claude_command(config),
        &[],
    )
}

fn repo_claude_command(config: &Config) -> String {
    launch_command(
        &config.default_program_command(),
        None,
        &config.default_program_autonomous_args(),
    )
}

pub fn kill_agent(repo: &RepoNode, agent: &AgentNode, force: bool) -> Result<()> {
    let parent = agent
        .worktree_path
        .canonicalize()
        .unwrap_or_else(|_| agent.worktree_path.clone());
    if git::list_worktrees(&repo.path)?
        .into_iter()
        .any(|worktree| {
            let path = worktree.path.canonicalize().unwrap_or(worktree.path);
            path != parent && path.starts_with(&parent)
        })
    {
        return Err(crate::Error::ChildWorktreesPresent(
            agent.worktree_path.clone(),
        ));
    }
    let worktree_exists = agent.worktree_path.exists();
    if !force && worktree_exists && !git::tracked_tree_is_clean(&agent.worktree_path)? {
        return Err(crate::Error::DirtyWorktree(agent.worktree_path.clone()));
    }

    let server = tmux::TmuxServer::default_server();
    let _ = tmux::kill_session(&server, &agent.tmux_session);
    let _ = tmux::kill_session(&server, &shell_session_name(agent));

    if worktree_exists {
        git::remove_worktree(&repo.path, &agent.worktree_path, force)
    } else {
        // The worktree directory is already gone (e.g. a dead agent whose
        // directory was deleted out from under robco). `git worktree remove`
        // would fail on the missing path, so just prune the stale administrative
        // entry; the caller then drops the registry row.
        git::prune_worktrees(&repo.path)
    }
}

fn shell_program() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
}

#[cfg(test)]
mod tests;
