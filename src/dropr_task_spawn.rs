//! CLI (`robco spawn --dropr-task`) and MCP (`robco_agent_create`'s
//! `dropr_task` argument) entry point for launching a worker from a dropr
//! task (dropr:540): resolves the repo, its dropr workspace, and the bare
//! task reference the operator supplied, then hands off to the same
//! claim/prompt/create/release sequence the TUI's `n` key uses
//! (`agent::dropr_task::launch`). The TUI does not go through this file — it
//! already holds the resolved repo, workspace id, and task row from its own
//! cached `task_list` fetch before the operator ever presses a key.

use crate::{
    Error, Result,
    agent::{
        self,
        dropr_task::{DroprTaskLaunch, LaunchError, launch},
    },
    config::Config,
    dropr::{self, TaskLookup},
    overseer::{exec::COMMAND_TIMEOUT, session::env::SessionEnv},
    registry::Registry,
    spawn::{self, SpawnOutcome, persist_child, worker_env},
};

/// dropr claim identity used by every in-process, operator-issued dropr-task
/// launch that reaches this file — `robco spawn --dropr-task` and
/// `robco_agent_create`'s `dropr_task` argument. Distinct from the Overseer
/// daemon's own `OVERSEER_AGENT_ID` (a dispatch decision) and from the TUI's
/// own `DIRECT_LAUNCH_AGENT_ID` (an in-process launch that is not this one).
const DROPR_TASK_SPAWN_AGENT_ID: &str = "robco-spawn";

#[allow(clippy::too_many_arguments)]
pub fn spawn_dropr_task_in_repo(
    repo_selector: &str,
    task_ref: &str,
    explicit_title: Option<&str>,
    explicit_prompt: Option<&str>,
    explicit_name_slug: Option<&str>,
    parent_agent_id: Option<&str>,
    extra_args: &[String],
    autonomous: bool,
    config: &Config,
) -> Result<SpawnOutcome> {
    if explicit_title.is_some() || explicit_prompt.is_some() || explicit_name_slug.is_some() {
        return Err(Error::DroprTaskSpawnConflict);
    }

    let registry = Registry::load()?;
    let repo = spawn::resolve_repo(&registry, repo_selector)?.clone();
    let remote = repo
        .remote_url
        .clone()
        .ok_or_else(|| Error::DroprTaskNoWorkspace(repo.name.clone()))?;
    let (overlay, _) = dropr::DroprOverlay::load_with_status_timeout(COMMAND_TIMEOUT);
    let workspace = overlay
        .find_by_repo_url(&remote)
        .ok_or_else(|| Error::DroprTaskNoWorkspace(repo.name.clone()))?;

    let task_ref = normalize_task_ref(task_ref);
    let candidate = match dropr::lookup_task(&workspace.id, &task_ref, COMMAND_TIMEOUT) {
        TaskLookup::Found(candidate) => candidate,
        TaskLookup::NotFound => return Err(Error::DroprTaskNotFound(task_ref)),
        TaskLookup::Unavailable => return Err(Error::DroprUnavailable),
    };

    let subtasks = if candidate.child_count > 0 {
        let fetched = dropr::fetch_subtasks(&workspace.id, &candidate.id, COMMAND_TIMEOUT);
        if fetched.is_empty() {
            return Err(Error::DroprSubtasksUnconfirmed(candidate.display_id));
        }
        fetched
    } else {
        Vec::new()
    };

    let blocked_env = if autonomous {
        agent::env::autonomous_env(&config.overseer.worker_env_blocklist)
    } else {
        Vec::new()
    };
    let launch_env = worker_env(blocked_env, &SessionEnv::resolve(config));

    let child = launch(DroprTaskLaunch {
        repo: &repo,
        config,
        workspace_id: &workspace.id,
        candidate: &candidate,
        subtasks: &subtasks,
        claim_agent_id: DROPR_TASK_SPAWN_AGENT_ID,
        parent_agent_id,
        extra_args,
        extra_env: &launch_env,
    })
    .map_err(|err| launch_error(&workspace.id, &task_ref, &candidate.id, err))?;

    let outcome = SpawnOutcome::from(&child);
    persist_child(&repo.path, child, &outcome)?;
    Ok(outcome)
}

/// Turns a launch failure into a `crate::Error` that names which of the two
/// operator-visible problems happened: the task not resolving at all, versus
/// another agent already holding its claim. A `"locked"` refusal reason gets
/// one follow-up read to name the holder; every other reason is surfaced
/// verbatim rather than as a generic failure.
fn launch_error(workspace_id: &str, task_ref: &str, task_id: &str, err: LaunchError) -> Error {
    match err {
        LaunchError::ClaimRefused(reason) if reason == "locked" => {
            let holder = dropr::task_claim(workspace_id, task_id, COMMAND_TIMEOUT)
                .and_then(|claim| claim.claimed_by_agent_id)
                .filter(|holder| !holder.is_empty());
            claimed_error(task_ref, holder)
        }
        LaunchError::ClaimRefused(reason) => refused_error(task_ref, reason),
        LaunchError::DroprUnreachable => Error::DroprUnavailable,
        LaunchError::Spawn(err) => err,
    }
}

fn claimed_error(task_ref: &str, holder: Option<String>) -> Error {
    Error::DroprTaskClaimed {
        task_ref: task_ref.to_string(),
        holder,
    }
}

fn refused_error(task_ref: &str, reason: String) -> Error {
    Error::DroprTaskClaimRefused {
        task_ref: task_ref.to_string(),
        reason,
    }
}

/// Accepts `538` and `#538` alike — dropr's own `task_list` / `task_next`
/// only resolve the `#`-prefixed display id form for a bare number (a plain
/// `538` answers `task not found`, confirmed live against a real workspace).
fn normalize_task_ref(raw: &str) -> String {
    let trimmed = raw.trim();
    if !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit()) {
        format!("#{trimmed}")
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
#[path = "dropr_task_spawn_tests.rs"]
mod tests;
