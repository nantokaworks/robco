use std::path::Path;

use crate::model::{AgentNode, RepoNode};

pub(super) fn is_slot_worktree(
    path: &Path,
    branch: Option<&str>,
    tracked_agents: &[AgentNode],
) -> bool {
    slot_owner(path, branch, tracked_agents).is_some()
        || branch.is_some_and(producer_branch_is_slot)
}

pub(in crate::ui) fn slot_owner(
    path: &Path,
    branch: Option<&str>,
    tracked_agents: &[AgentNode],
) -> Option<usize> {
    tracked_agents
        .iter()
        .position(|tracked| matches_slot(path, branch, tracked))
}

/// Removes slot worktrees adopted as top-level agents, not nested children.
pub(super) fn prune_top_level_slot_agents(repo: &mut RepoNode) {
    let tracked_agents = repo.agents.clone();
    repo.agents.retain(|candidate| {
        !is_slot_worktree(
            &candidate.worktree_path,
            Some(&candidate.branch),
            &tracked_agents,
        )
    });
}

fn matches_slot(path: &Path, branch: Option<&str>, tracked: &AgentNode) -> bool {
    branch.is_some_and(|candidate| branch_is_slot(candidate, &tracked.branch))
        || directory_is_slot(path, branch, &tracked.worktree_path)
}

fn producer_branch_is_slot(candidate: &str) -> bool {
    candidate
        .strip_prefix("slot/task-")
        .and_then(|suffix| suffix.split_once('-'))
        .is_some_and(|(task, name)| is_nonempty_digits(task) && !name.is_empty())
}

fn branch_is_slot(candidate: &str, tracked: &str) -> bool {
    candidate
        .strip_prefix(tracked)
        .and_then(|suffix| suffix.strip_prefix("-slot-"))
        .is_some_and(is_nonempty_digits)
}

fn directory_is_slot(candidate: &Path, branch: Option<&str>, tracked: &Path) -> bool {
    let (Some(candidate_parent), Some(tracked_parent)) = (candidate.parent(), tracked.parent())
    else {
        return false;
    };
    if super::discovery::path_key(candidate_parent) != super::discovery::path_key(tracked_parent) {
        return false;
    }
    let Some(candidate) = candidate.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(tracked) = tracked.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if managed_task_directory(tracked)
        && branch.is_none_or(producer_branch_is_slot)
        && candidate
            .strip_prefix(tracked)
            .and_then(|suffix| suffix.strip_prefix("_slot_"))
            .is_some_and(|name| !name.is_empty())
    {
        return true;
    }
    let Some((prefix, random)) = tracked.rsplit_once('_') else {
        return false;
    };
    if random.is_empty() || !prefix_is_managed_task(prefix) {
        return false;
    }
    candidate
        .strip_prefix(prefix)
        .and_then(|suffix| suffix.strip_prefix("_slot"))
        .is_some_and(is_nonempty_digits)
}

fn managed_task_directory(directory: &str) -> bool {
    prefix_is_managed_task(directory)
        || directory
            .rsplit_once('_')
            .is_some_and(|(prefix, suffix)| !suffix.is_empty() && prefix_is_managed_task(prefix))
}

fn prefix_is_managed_task(prefix: &str) -> bool {
    prefix.rsplit_once("_task-").is_some_and(|(repo, task)| {
        let valid_task = task.split_once('-').map_or_else(
            || is_nonempty_digits(task),
            |(task_id, slug)| is_nonempty_digits(task_id) && !slug.is_empty(),
        );
        !repo.is_empty() && valid_task
    })
}

fn is_nonempty_digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
#[path = "slots_tests.rs"]
mod tests;
