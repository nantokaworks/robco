use std::fs;

use chrono::Local;
use nanoid::nanoid;

use super::env::{
    RecoveredIdentity, agent_env, claude_session_id, launch_command, session_id_args,
};
use super::hooks::write_report_hooks;
use super::naming::{
    leading_task_number, naming_slug, profile_name, resolve_branch_prefix, worker_branch_name,
};
use crate::{
    Error, Result,
    config::Config,
    git,
    model::{AgentNode, RepoNode},
    overseer::OVERSEER_AGENT_ID,
    tmux,
};

/// The repository's own default branch — new work bases on it, never on a
/// hardcoded `main` (dropr:503). `Err` rather than a guess when it cannot be
/// resolved: a worker's base commit has to come from somewhere real.
fn resolve_base_branch(repo: &RepoNode) -> Result<String> {
    git::default_branch(&repo.path)?
        .ok_or_else(|| Error::DefaultBranchUnresolved(repo.path.clone()))
}

/// Decide a new worker's Overseer parentage.
///
/// A caller-supplied parent is never touched: it may be another agent's id
/// (a subagent spawned by `robco new`), and overwriting it would break the
/// identity tree `model::agent_order` builds from, with no way to restore the
/// old value later.
///
/// A worker created with no parent has nothing to lose, so it starts enrolled
/// with the Overseer: `parent_agent_id` becomes the Overseer's id.
fn enroll_with_overseer(parent_agent_id: Option<&str>) -> Option<String> {
    match parent_agent_id {
        Some(parent) => Some(parent.to_string()),
        None => Some(OVERSEER_AGENT_ID.to_string()),
    }
}

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
) -> Result<AgentNode> {
    let id = nanoid!(8);
    let task_number = leading_task_number(name_slug);
    let slug = naming_slug(title, name_slug);
    let branch = worker_branch_name(config, &repo.name, title, name_slug);
    // Base new work on `origin/<default>`, fetched fresh — not on whatever
    // happens to be checked out in the primary worktree, which may be an
    // operator's own branch mid-work.
    let base_branch = resolve_base_branch(repo)?;
    let base_commit = git::remote_branch_commit(&repo.path, &base_branch)?;
    let worktree_path = config
        .worktree_root
        .join(format!("{}_{}_{}", repo.name, slug, &id[..6]));
    let tmux_session = tmux::session_name(&config.tmux_session_prefix, &repo.name, &slug);

    fs::create_dir_all(&config.worktree_root)?;
    git::add_worktree(&repo.path, &worktree_path, &branch, &base_commit)?;
    let program = config.default_program_command();
    // Every worker robco creates gets its report hooks here, in the one
    // place all three creation paths (`robco spawn`, TUI agent creation, the
    // dropr-task `n` key) actually share — see the dropr:532 decision
    // scribble on this task for why a per-caller closure was the wrong place.
    write_report_hooks(&worktree_path, &program)?;
    let claude_session_id = claude_session_id(&program);
    let mut launch_args = session_id_args(claude_session_id.as_deref());
    launch_args.extend_from_slice(extra_args);
    let command = launch_command(&program, initial_prompt, &launch_args);
    let parent_agent_id = enroll_with_overseer(parent_agent_id);
    let mut owned_env = agent_env(&id, parent_agent_id.as_deref())
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
        parent_agent_id,
        title: title.to_string(),
        task_number,
        worktree_path,
        branch,
        base_commit,
        program,
        claude_session_id,
        profile: profile_name(config),
        tmux_session,
        created_at: now,
        updated_at: now,
        status: Default::default(),
        worktree_missing: false,
        merge_error: None,
        last_capture: None,
        last_spinner: None,
        last_change_at: None,
        last_auto_accept_at: None,
        shell_working: false,
        mcp_active: false,
        pane_pid: None,
        tracked_command: None,
        subagents: Vec::new(),
        children: Vec::new(),
    })
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
        // Adoption recovers `parent_agent_id` from the live session as-is,
        // unlike `create_agent_with_launch`: a `None` parent here is NOT
        // enrolled. A session that reports no parent may be a worker created
        // before enrolment existed, or one an operator deliberately detached.
        // Adoption cannot tell those apart, so it leaves the worker unowned.
        parent_agent_id,
        title: label,
        // Adoption never learns a dropr task number: nothing here carries the
        // slug `create_agent_with_launch` derived one from, and a re-adopted
        // worker's row rendering exactly as an unowned one's is deliberate
        // (see `crate::model::AgentNode::task_number`).
        task_number: None,
        worktree_path,
        branch: branch.unwrap_or_else(|| "(detached)".to_string()),
        base_commit: head.unwrap_or_default(),
        program: config.default_program_command(),
        claude_session_id: None,
        profile: profile_name(config),
        tmux_session,
        created_at: now,
        updated_at: now,
        status: Default::default(),
        worktree_missing: false,
        merge_error: None,
        last_capture: None,
        last_spinner: None,
        last_change_at: None,
        last_auto_accept_at: None,
        shell_working: false,
        mcp_active: false,
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

#[cfg(test)]
mod tests;
