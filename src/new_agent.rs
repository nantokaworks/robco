use crate::{
    Error, Result, agent,
    cli::NewArgs,
    config::{Config, ENV_AGENT_ID},
    registry::Registry,
};

pub fn run(args: NewArgs, config: &Config) -> Result<()> {
    let parent_id = std::env::var(ENV_AGENT_ID)
        .ok()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .ok_or(Error::NewOutsideAgentSession)?;

    let registry = Registry::load()?;
    let repo = registry
        .repos
        .iter()
        .find(|repo| repo.agents.iter().any(|agent| agent.id == parent_id))
        .ok_or_else(|| Error::ParentAgentNotFound(parent_id.clone()))?;
    let repo_path = repo.path.clone();
    let child = agent::create_agent(
        repo,
        &args.title,
        args.prompt.as_deref(),
        config,
        Some(&parent_id),
    )?;

    let missing_worktree_path = child.worktree_path.clone();
    let missing_tmux_session = child.tmux_session.clone();
    let mut repo_index = None;
    let registry = Registry::locked_update(|registry| {
        repo_index = registry
            .repos
            .iter()
            .position(|repo| repo.path == repo_path);
        if let Some(repo_index) = repo_index {
            registry.repos[repo_index].agents.push(child);
        }
    })?;
    let repo_index = repo_index.ok_or_else(|| Error::CreatedChildRepoMissing {
        worktree_path: missing_worktree_path,
        tmux_session: missing_tmux_session,
    })?;
    let child = registry.repos[repo_index]
        .agents
        .last()
        .expect("child was just inserted");
    println!("id: {}", child.id);
    println!("branch: {}", child.branch);
    println!("worktree: {}", child.worktree_path.display());
    println!("tmux: {}", child.tmux_session);
    Ok(())
}
