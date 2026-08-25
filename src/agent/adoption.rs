use chrono::Local;
use nanoid::nanoid;

use super::env::RecoveredIdentity;
use super::naming::{profile_name, resolve_branch_prefix};
use crate::{
    config::Config,
    model::{AgentNode, RepoNode},
    tmux,
};

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
/// [`super::creation::create_agent`] stores the raw title, not the prefixed
/// branch. Returns whether any title changed so the caller knows to persist
/// the registry.
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
#[path = "adoption/tests.rs"]
mod tests;
