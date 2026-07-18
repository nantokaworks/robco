use std::fs;

use chrono::Local;
use nanoid::nanoid;

use crate::{
    Result,
    config::Config,
    git,
    model::{AgentNode, ManagementMode, RepoNode},
    overseer::is_overseer_child,
    tmux,
};

pub mod env;
pub use env::RecoveredIdentity;
use env::{agent_env, launch_command};

pub fn create_agent(
    repo: &RepoNode,
    title: &str,
    initial_prompt: Option<&str>,
    config: &Config,
    parent_agent_id: Option<&str>,
) -> Result<AgentNode> {
    create_agent_with_launch(
        repo,
        title,
        None,
        initial_prompt,
        config,
        parent_agent_id,
        &[],
        &[],
        |_| Ok(()),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn create_agent_with_launch(
    repo: &RepoNode,
    title: &str,
    name_slug: Option<&str>,
    initial_prompt: Option<&str>,
    config: &Config,
    parent_agent_id: Option<&str>,
    extra_args: &[String],
    extra_env: &[(String, String)],
    prepare_worktree: impl FnOnce(&std::path::Path) -> Result<()>,
) -> Result<AgentNode> {
    let id = nanoid!(8);
    let name_slug = naming_slug(title, name_slug);
    let branch = format!("{}{}", resolve_branch_prefix(config, &repo.name), name_slug);
    let base_commit = git::head_commit(&repo.path)?;
    let worktree_path =
        config
            .worktree_root
            .join(format!("{}_{}_{}", repo.name, name_slug, &id[..6]));
    let tmux_session = tmux::session_name(&config.tmux_session_prefix, &repo.name, &name_slug);

    fs::create_dir_all(&config.worktree_root)?;
    git::add_worktree(&repo.path, &worktree_path, &branch, &base_commit)?;
    prepare_worktree(&worktree_path)?;
    let program = config.default_program_command();
    let command = launch_command(&program, initial_prompt, extra_args);
    let mut owned_env = agent_env(&id, parent_agent_id)
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect::<Vec<_>>();
    owned_env.extend(extra_env.iter().cloned());
    let launch_env = owned_env
        .iter()
        .map(|(key, value)| (key.as_str(), value.clone()))
        .collect::<Vec<_>>();
    tmux::new_session(&tmux_session, &worktree_path, &command, &launch_env)?;

    let now = Local::now();
    Ok(AgentNode {
        id,
        parent_agent_id: parent_agent_id.map(str::to_string),
        management: if is_overseer_child(parent_agent_id) {
            ManagementMode::Auto
        } else {
            ManagementMode::Manual
        },
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
        worktree_missing: false,
        merge_error: None,
        last_capture: None,
        last_change_at: None,
        last_auto_accept_at: None,
        shell_working: false,
        pane_pid: None,
        tracked_command: None,
        subagents: Vec::new(),
        children: Vec::new(),
    })
}

fn naming_slug(title: &str, explicit: Option<&str>) -> String {
    let sanitized = tmux::sanitize_target_part(explicit.unwrap_or(title));
    let capped = cap_name_slug(&sanitized);
    if capped.is_empty() {
        "agent".to_string()
    } else {
        capped
    }
}

fn cap_name_slug(raw: &str) -> String {
    const MAX_CHARS: usize = 32;

    let hard_cap: String = raw.chars().take(MAX_CHARS).collect();
    let mut capped = if raw.chars().count() > MAX_CHARS {
        hard_cap.rfind('-').map_or_else(
            || hard_cap.clone(),
            |boundary| hard_cap[..boundary].to_string(),
        )
    } else {
        hard_cap.clone()
    };
    capped.truncate(capped.trim_end_matches('-').len());
    if capped.is_empty() && !raw.is_empty() {
        hard_cap
    } else {
        capped
    }
}

/// Build an [`AgentNode`] for a worktree that already exists on disk but is not
/// yet tracked (created outside robco). No tmux session is launched — the AI
/// starts only when the user attaches — so the session is named but assumed
/// absent until then. `existing_session` is a live AI session already running
/// in this worktree (found by cwd, e.g. via [`crate::tmux::find_session_by_cwd`]);
/// when present it binds to that session instead of spawning a duplicate.
pub fn adopt_worktree(
    repo: &RepoNode,
    config: &Config,
    worktree_path: std::path::PathBuf,
    branch: Option<String>,
    head: Option<String>,
    existing_session: Option<String>,
    recovered_identity: Option<RecoveredIdentity>,
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
    let (id, parent_agent_id) = recovered_identity
        .map(|identity| (identity.id, identity.parent_agent_id))
        .unwrap_or_else(|| (nanoid!(8), None));
    AgentNode {
        id,
        management: ManagementMode::Manual,
        parent_agent_id,
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
        worktree_missing: false,
        merge_error: None,
        last_capture: None,
        last_change_at: None,
        last_auto_accept_at: None,
        shell_working: false,
        pane_pid: None,
        tracked_command: None,
        subagents: Vec::new(),
        children: Vec::new(),
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
        &agent_env(&agent.id, agent.parent_agent_id.as_deref()),
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
        &agent_env(&agent.id, agent.parent_agent_id.as_deref()),
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

    let _ = tmux::kill_session(&agent.tmux_session);
    let _ = tmux::kill_session(&shell_session_name(agent));

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

fn shell_program() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
}

#[cfg(test)]
mod tests;
