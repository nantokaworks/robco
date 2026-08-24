//! The claim → prompt → create → release sequence a dropr-task launch needs
//! (dropr:540), shared by the TUI `n` key (`ui::actions::dropr_task_worker`),
//! `robco spawn --dropr-task` and the MCP `robco_agent_create` tool's
//! `dropr_task` argument (`crate::dropr_task_spawn`) — one implementation of
//! the four things a launch does together instead of three: claim the task
//! (`dropr::claim_task`), name the worker `"{display_id} {title}"`, build its
//! prompt (`worker_prompt`), and hand the claim back if the worker never
//! starts (`dropr::release_claim`).
//!
//! What is *not* shared: resolving a bare task reference into a full
//! [`DroprTaskCandidate`] and its subtasks. The TUI already holds both from
//! its own cached `task_list` fetch; the CLI and MCP paths only ever receive
//! a task id, so they resolve it themselves (`crate::dropr_task_spawn`)
//! before calling in here.

use crate::{
    config::Config,
    dropr::{self, DroprTaskCandidate},
    model::{AgentNode, RepoNode},
    overseer::{exec::COMMAND_TIMEOUT, templates::worker_prompt},
};

use super::create_agent_with_launch;

/// Everything a claimed launch needs, already resolved by the caller.
pub(crate) struct DroprTaskLaunch<'a> {
    pub repo: &'a RepoNode,
    pub config: &'a Config,
    pub workspace_id: &'a str,
    pub candidate: &'a DroprTaskCandidate,
    pub subtasks: &'a [dropr::Subtask],
    /// dropr identity the claim (and its release on failure) is taken under.
    pub claim_agent_id: &'a str,
    pub parent_agent_id: Option<&'a str>,
    pub extra_args: &'a [String],
    pub extra_env: &'a [(String, String)],
}

/// Why a launch stopped. Distinct from [`crate::Error`]: a claim refusal or
/// an unreachable dropr is not an agent-creation failure, and each caller
/// (the TUI's locale-driven messages, the CLI/MCP's `crate::Error`) renders
/// it differently.
#[derive(Debug)]
pub(crate) enum LaunchError {
    /// dropr answered and declined the claim; carries its reason (e.g.
    /// `"locked"`, `"blocked"`, `"dependency_blocked"`).
    ClaimRefused(String),
    /// The claim call never reached a verdict.
    DroprUnreachable,
    /// The claim was taken, but the agent could not be created. The claim has
    /// already been handed back by the time this is returned.
    Spawn(crate::Error),
}

pub(crate) fn launch(request: DroprTaskLaunch) -> Result<AgentNode, LaunchError> {
    launch_with(request, dropr::claim_task, dropr::release_claim)
}

/// The generic core `launch` wraps with the real dropr calls: parameterized
/// over `claim` and `release` so a test can exercise the claim/create/release
/// sequence without a live `dropr` binary, the same way
/// `dropr::repo_tasks::fetch_within` injects its own asker.
fn launch_with<C, R>(
    request: DroprTaskLaunch,
    claim: C,
    release: R,
) -> Result<AgentNode, LaunchError>
where
    C: FnOnce(&str, &str, &str, std::time::Duration) -> dropr::ClaimAttempt,
    R: FnOnce(&str, &str, &str, std::time::Duration) -> bool,
{
    match claim(
        request.workspace_id,
        &request.candidate.id,
        request.claim_agent_id,
        COMMAND_TIMEOUT,
    ) {
        dropr::ClaimAttempt::Claimed => {}
        dropr::ClaimAttempt::Refused(reason) => return Err(LaunchError::ClaimRefused(reason)),
        dropr::ClaimAttempt::Unavailable => return Err(LaunchError::DroprUnreachable),
    }

    let title = format!(
        "{} {}",
        request.candidate.display_id, request.candidate.title
    );
    let prompt = worker_prompt(
        &request.candidate.display_id,
        &request.candidate.id,
        &request.candidate.title,
        &request.repo.name,
        request.subtasks,
        request.config.language.as_deref(),
        request.config.overseer.worker_prompt_template.as_deref(),
    );

    create_agent_with_launch(
        request.repo,
        &title,
        None,
        Some(&prompt),
        request.config,
        request.parent_agent_id,
        request.extra_args,
        request.extra_env,
    )
    .map_err(|err| {
        // The claim was taken for a worker that never started; holding it
        // would park the task away from the next operator or dispatch pass.
        let _ = release(
            request.workspace_id,
            &request.candidate.id,
            request.claim_agent_id,
            COMMAND_TIMEOUT,
        );
        LaunchError::Spawn(err)
    })
}

#[cfg(test)]
#[path = "dropr_task_tests.rs"]
mod tests;
