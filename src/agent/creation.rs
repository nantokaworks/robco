use std::fs;

use chrono::Local;
use nanoid::nanoid;

use super::env::{agent_env, claude_session_id, launch_command, session_id_args};
use super::hooks::write_report_hooks;
use super::naming::{leading_task_number, naming_slug, profile_name, worker_branch_name};
use crate::{
    Error, Result,
    config::Config,
    git,
    model::{AgentNode, RepoNode},
    overseer::{OVERSEER_AGENT_ID, logging, session::env::SessionEnv},
    spawn::worker_env,
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

/// The full launch environment every TUI-originated worker receives,
/// resolved together so no call site can apply one piece without the others:
/// the profile's `autonomous_args`, the env blocklist paired with them
/// (dropr:538), and the operator's session-credential channel (`SessionEnv`,
/// widened in by dropr:546). The operator's call (dropr:538): a worker
/// started from the TUI is always launched the same way
/// `robco spawn --autonomous` is — this is not a config switch. A profile
/// with no configured `autonomous_args` (`~/.robco/config.json` has carried
/// one before) resolves the flag half of the pair to empty, which
/// `create_agent_with_launch` already treats as a normal, non-autonomous
/// launch rather than an error.
///
/// This is deliberately not what `robco spawn` / `robco spawn --dropr-task`
/// use (`crate::spawn::worker_env`, driven by the CLI `--autonomous` flag):
/// those callers resolve their own environment so a deliberately
/// non-autonomous CLI spawn stays expressible. `tui_launch_env` is for the
/// two TUI launch paths only — `create_agent` (`ui/input.rs`'s plain
/// creation) and the dropr-task `n` key
/// (`ui::actions::dropr_task_worker::run_launch`) — where dropr:538 already
/// decided there is no non-autonomous switch to preserve.
pub(crate) fn tui_launch_env(config: &Config) -> (Vec<String>, Vec<(String, String)>) {
    let args = config.default_program_autonomous_args();
    let blocked_env = if args.is_empty() {
        Vec::new()
    } else {
        super::env::autonomous_env(&config.overseer.worker_env_blocklist)
    };
    let env = worker_env(blocked_env, &SessionEnv::resolve(config));
    (args, env)
}

pub fn create_agent(
    repo: &RepoNode,
    title: &str,
    initial_prompt: Option<&str>,
    config: &Config,
    parent_agent_id: Option<&str>,
) -> Result<AgentNode> {
    let (autonomous_args, extra_env) = tui_launch_env(config);
    create_agent_with_launch(
        repo,
        title,
        None,
        initial_prompt,
        config,
        parent_agent_id,
        &autonomous_args,
        &extra_env,
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
    if let Err(error) =
        tmux::new_worker_session(&tmux_session, &worktree_path, &command, &launch_env)
    {
        // These two kinds are the whole point of dropr:554: a launch that
        // failed *this* way used to report nothing beyond a bare "session is
        // dead" once the daemon noticed, minutes later, with the pane's own
        // output already gone. Recording it here — the moment it is known —
        // is what gets it into the decision log at all.
        if matches!(
            error,
            Error::WorkerLaunchCrashed { .. } | Error::WorkerLaunchWrongCwd { .. }
        ) {
            let _ = logging::log_message(None, &format!("{tmux_session}: {error}"));
        }
        return Err(error);
    }

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

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tui_launch_env_tests;
