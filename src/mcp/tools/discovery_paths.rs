use std::path::{Component, Path, PathBuf};

use crate::model::AgentNode;

pub(super) fn matches_slot(path: &Path, branch: Option<&str>, tracked: &AgentNode) -> bool {
    branch.is_some_and(|candidate| branch_is_slot(candidate, &tracked.branch))
        || directory_is_slot(path, branch, &tracked.worktree_path)
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
    if path_key(candidate_parent) != path_key(tracked_parent) {
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

fn producer_branch_is_slot(candidate: &str) -> bool {
    candidate
        .strip_prefix("slot/task-")
        .and_then(|suffix| suffix.split_once('-'))
        .is_some_and(|(task, name)| is_nonempty_digits(task) && !name.is_empty())
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

pub(super) fn path_is_strictly_inside(path: &Path, parent: &Path) -> bool {
    let path = path.canonicalize().unwrap_or_else(|_| normalize_path(path));
    let parent = parent
        .canonicalize()
        .unwrap_or_else(|_| normalize_path(parent));
    path != parent && path.starts_with(parent)
}

pub(super) fn is_managed_worktree(path: &Path, root: &Path) -> bool {
    let path = path.canonicalize().unwrap_or_else(|_| normalize_path(path));
    let root = root.canonicalize().unwrap_or_else(|_| normalize_path(root));
    path.starts_with(root)
}

pub(super) fn path_key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| normalize_path(path))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component),
        }
    }
    normalized
}
