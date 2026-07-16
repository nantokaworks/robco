use crate::{
    Error, Result,
    chief::CHIEF_AGENT_ID,
    cli::NewArgs,
    config::{Config, ENV_AGENT_ID},
    registry::Registry,
    spawn,
};

pub fn run(args: NewArgs, config: &Config) -> Result<()> {
    let parent_id = std::env::var(ENV_AGENT_ID)
        .ok()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .ok_or(Error::NewOutsideAgentSession)?;

    let registry = Registry::load()?;
    let repo_path = if parent_id == CHIEF_AGENT_ID {
        let cwd = std::env::current_dir()?.canonicalize()?;
        registry.repos.iter().find(|repo| {
            repo.path
                .canonicalize()
                .unwrap_or_else(|_| repo.path.clone())
                == cwd
        })
    } else {
        registry
            .repos
            .iter()
            .find(|repo| repo.agents.iter().any(|agent| agent.id == parent_id))
    }
    .map(|repo| repo.path.clone())
    .ok_or_else(|| Error::ParentAgentNotFound(parent_id.clone()))?;
    let outcome = spawn::spawn_in_repo(
        &repo_path.to_string_lossy(),
        &args.title,
        args.prompt.as_deref(),
        Some(&parent_id),
        &[],
        config,
    )?;
    println!("id: {}", outcome.id);
    println!("branch: {}", outcome.branch);
    println!("worktree: {}", outcome.worktree_path.display());
    println!("tmux: {}", outcome.tmux_session);
    Ok(())
}
