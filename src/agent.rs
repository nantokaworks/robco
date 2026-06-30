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

pub fn create_agent(
    repo: &RepoNode,
    title: &str,
    initial_prompt: Option<&str>,
    config: &Config,
) -> Result<AgentNode> {
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
    let program = config.default_program_command();
    let command = launch_command(&program, initial_prompt);
    tmux::new_session(&tmux_session, &worktree_path, &command)?;

    let now = Local::now();
    Ok(AgentNode {
        id,
        title: title.to_string(),
        worktree_path,
        branch,
        base_commit,
        program,
        profile: profile_name(config),
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

pub fn ship_agent(agent: &AgentNode) -> Result<()> {
    git::add_all(&agent.worktree_path)?;
    git::commit(&agent.worktree_path, &format!("robco: {}", agent.title))?;
    git::push_branch(&agent.worktree_path, &agent.branch)
}

fn profile_name(config: &Config) -> Option<String> {
    config
        .profiles
        .iter()
        .find(|profile| profile.name == config.default_program)
        .map(|profile| profile.name.clone())
}

fn launch_command(program: &str, initial_prompt: Option<&str>) -> String {
    match initial_prompt
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
    {
        Some(prompt) => format!("{program} {}", shell_quote(prompt)),
        None => program.to_string(),
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_initial_prompt_for_shell_command() {
        assert_eq!(
            launch_command("claude", Some("fix Bob's bug")),
            "claude 'fix Bob'\\''s bug'"
        );
    }
}
