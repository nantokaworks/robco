use std::fs;

use chrono::Local;
use nanoid::nanoid;

use crate::{
    Result,
    config::{Config, ENV_AGENT_ID},
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
    let branch = format!(
        "{}{}",
        resolve_branch_prefix(config, &repo.name),
        clean_title
    );
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
    tmux::new_session(
        &tmux_session,
        &worktree_path,
        &command,
        &[(ENV_AGENT_ID, id.clone())],
    )?;

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
        last_auto_accept_at: None,
        shell_working: false,
    })
}

/// Build an [`AgentNode`] for a worktree that already exists on disk but is not
/// yet tracked (created outside robco). No tmux session is launched — the AI
/// starts only when the user attaches — so the session is named but assumed
/// absent until then. `existing_session` is a live AI session already running
/// in this worktree (found by cwd, e.g. via [`crate::tmux::find_session_by_cwd`]);
/// when present the agent binds to it instead of the derived name, so adoption
/// reattaches to the surviving chat rather than spawning a duplicate.
pub fn adopt_worktree(
    repo: &RepoNode,
    config: &Config,
    worktree_path: std::path::PathBuf,
    branch: Option<String>,
    head: Option<String>,
    existing_session: Option<String>,
) -> AgentNode {
    // git forbids two worktrees on the same branch, so the branch (or the
    // directory name for a detached worktree) is a stable per-repo identifier.
    // Branches robco created itself are `<prefix><title>`; strip the prefix so
    // the adopted agent resolves to the same tmux session name `create_agent`
    // produced and reattaches to a still-running session instead of spawning a
    // duplicate one.
    let label = branch
        .clone()
        .map(|branch| {
            let prefix = resolve_branch_prefix(config, &repo.name);
            branch
                .strip_prefix(&prefix)
                .map(str::to_string)
                .unwrap_or(branch)
        })
        .unwrap_or_else(|| {
            worktree_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("worktree")
                .to_string()
        });
    let clean_label = tmux::sanitize_target_part(&label);
    let tmux_session = existing_session.unwrap_or_else(|| {
        tmux::session_name(&config.tmux_session_prefix, &repo.name, &clean_label)
    });
    let now = Local::now();
    AgentNode {
        id: nanoid!(8),
        title: label,
        worktree_path,
        branch: branch.unwrap_or_else(|| "(detached)".to_string()),
        base_commit: head.unwrap_or_default(),
        program: config.default_program_command(),
        profile: profile_name(config),
        tmux_session,
        created_at: now,
        updated_at: now,
        status: Default::default(),
        last_capture: None,
        last_change_at: None,
        last_auto_accept_at: None,
        shell_working: false,
    }
}

/// Strip the resolved branch prefix from agent titles persisted before
/// [`adopt_worktree`] learned to do so. Such entries carry the full branch
/// name as their title (e.g. `dropr/support-open-claw` under the `dropr`
/// repo), which is redundant in the tree where the parent row already names
/// the repo. Only titles equal to their branch — the adoption artifact
/// signature — are rewritten; user-typed titles never match because
/// [`create_agent`] stores the raw title, not the prefixed branch. Returns
/// whether any title changed so the caller knows to persist the registry.
pub fn normalize_adopted_titles(repos: &mut [RepoNode], config: &Config) -> bool {
    let mut changed = false;
    for repo in repos {
        let prefix = resolve_branch_prefix(config, &repo.name);
        for agent in &mut repo.agents {
            if agent.title != agent.branch {
                continue;
            }
            if let Some(rest) = agent.title.strip_prefix(&prefix)
                && !rest.is_empty()
            {
                agent.title = rest.to_string();
                changed = true;
            }
        }
    }
    changed
}

pub fn restart_agent(agent: &AgentNode) -> Result<()> {
    let _ = tmux::kill_session(&agent.tmux_session);
    tmux::new_session(
        &agent.tmux_session,
        &agent.worktree_path,
        &agent.program,
        &[(ENV_AGENT_ID, agent.id.clone())],
    )
}

pub fn ensure_agent_session(agent: &AgentNode) -> Result<()> {
    if tmux::has_session(&agent.tmux_session)? {
        return Ok(());
    }

    tmux::new_session(
        &agent.tmux_session,
        &agent.worktree_path,
        &agent.program,
        &[(ENV_AGENT_ID, agent.id.clone())],
    )
}

pub fn shell_session_name(agent: &AgentNode) -> String {
    format!("{}-shell", agent.tmux_session)
}

pub fn ensure_shell_session(agent: &AgentNode) -> Result<()> {
    let session = shell_session_name(agent);
    if tmux::has_session(&session)? {
        return Ok(());
    }

    tmux::new_session(&session, &agent.worktree_path, &shell_program(), &[])
}

pub fn repo_shell_session_name(prefix: &str, repo: &RepoNode) -> String {
    format!("{}-shell", tmux::session_name(prefix, &repo.name, "main"))
}

pub fn repo_claude_session_name(prefix: &str, repo: &RepoNode) -> String {
    tmux::session_name(prefix, &repo.name, "main")
}

pub fn ensure_repo_shell_session(prefix: &str, repo: &RepoNode) -> Result<()> {
    let session = repo_shell_session_name(prefix, repo);
    if tmux::has_session(&session)? {
        return Ok(());
    }

    tmux::new_session(&session, &repo.path, &shell_program(), &[])
}

pub fn ensure_repo_claude_session(config: &Config, prefix: &str, repo: &RepoNode) -> Result<()> {
    let session = repo_claude_session_name(prefix, repo);
    if tmux::has_session(&session)? {
        return Ok(());
    }

    tmux::new_session(&session, &repo.path, &config.default_program_command(), &[])
}

pub fn kill_agent(repo: &RepoNode, agent: &AgentNode) -> Result<()> {
    let worktree_exists = agent.worktree_path.exists();
    if worktree_exists && !git::tracked_tree_is_clean(&agent.worktree_path)? {
        return Err(crate::Error::DirtyWorktree(agent.worktree_path.clone()));
    }

    let _ = tmux::kill_session(&agent.tmux_session);
    let _ = tmux::kill_session(&shell_session_name(agent));

    if worktree_exists {
        git::remove_worktree(&repo.path, &agent.worktree_path)
    } else {
        // The worktree directory is already gone (e.g. a dead agent whose
        // directory was deleted out from under robco). `git worktree remove`
        // would fail on the missing path, so just prune the stale administrative
        // entry; the caller then drops the registry row.
        git::prune_worktrees(&repo.path)
    }
}

fn resolve_branch_prefix(config: &Config, repo_name: &str) -> String {
    if let Some(prefix) = &config.branch_prefix {
        return prefix.clone();
    }
    // Derive `<repo>/` from the project name, sanitized so it is a valid git
    // ref. A pathological repo name (e.g. `...`) sanitizes to an empty string,
    // which would yield a `/`-prefixed (invalid) branch — fall back to the
    // historical default in that case.
    let sanitized = tmux::sanitize_target_part(repo_name);
    if sanitized.is_empty() {
        "robco/".to_string()
    } else {
        format!("{}/", sanitized)
    }
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

fn shell_program() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests;
