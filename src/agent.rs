use std::fs;

use chrono::Local;
use nanoid::nanoid;

use crate::{
    Result,
    config::Config,
    git,
    model::{AgentNode, RepoNode},
    tmux,
};

pub fn create_agent(repo: &RepoNode, title: &str, config: &Config) -> Result<AgentNode> {
    let id = nanoid!(8);
    let clean_title = tmux::sanitize_target_part(title);
    let branch = format!("{}{}", config.branch_prefix, clean_title);
    let base_commit = git::head_commit(&repo.path)?;
    let worktree_path =
        config
            .worktree_root
            .join(format!("{}_{}_{}", repo.name, clean_title, &id[..6]));
    let tmux_session = tmux::session_name(&config.tmux_session_prefix, &repo.name, &clean_title);

    fs::create_dir_all(&config.worktree_root)?;
    git::add_worktree(&repo.path, &worktree_path, &branch, &base_commit)?;
    tmux::new_session(&tmux_session, &worktree_path, &config.default_program)?;

    let now = Local::now();
    Ok(AgentNode {
        id,
        title: title.to_string(),
        worktree_path,
        branch,
        base_commit,
        program: config.default_program.clone(),
        tmux_session,
        created_at: now,
        updated_at: now,
        status: Default::default(),
        last_capture: None,
        last_change_at: None,
    })
}

pub fn restart_agent(agent: &AgentNode) -> Result<()> {
    let _ = tmux::kill_session(&agent.tmux_session);
    tmux::new_session(&agent.tmux_session, &agent.worktree_path, &agent.program)
}

pub fn kill_agent(repo: &RepoNode, agent: &AgentNode) -> Result<()> {
    if !git::tracked_tree_is_clean(&agent.worktree_path)? {
        return Err(crate::Error::DirtyWorktree(agent.worktree_path.clone()));
    }

    let _ = tmux::kill_session(&agent.tmux_session);
    git::remove_worktree(&repo.path, &agent.worktree_path)
}
