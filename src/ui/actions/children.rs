use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Local};

use crate::{
    agent,
    config::Config,
    git::{self, Worktree},
    model::{ChildWorktree, RepoNode},
};

use super::discovery::{is_managed_worktree, path_is_strictly_inside, path_key};

pub(super) fn reconcile(
    repo: &mut RepoNode,
    config: &Config,
    worktrees: Vec<Worktree>,
) -> (bool, bool) {
    let previous = child_paths(repo);
    for agent in &mut repo.agents {
        agent.children.clear();
    }
    let mut known: HashSet<String> = repo
        .agents
        .iter()
        .map(|agent| path_key(&agent.worktree_path))
        .collect();
    known.insert(path_key(&repo.path));
    let mut added = false;

    for worktree in worktrees {
        if known.contains(&path_key(&worktree.path)) {
            continue;
        }
        if let Some(parent) = repo
            .agents
            .iter()
            .position(|agent| path_is_strictly_inside(&worktree.path, &agent.worktree_path))
        {
            let child = probe(repo, parent, worktree, config);
            repo.agents[parent].children.push(child);
            continue;
        }
        if !is_managed_worktree(&worktree.path, &config.worktree_root) {
            continue;
        }
        known.insert(path_key(&worktree.path));
        let session = crate::tmux::find_session_by_cwd(&config.tmux_session_prefix, &worktree.path);
        let adopted = agent::adopt_worktree(
            repo,
            config,
            worktree.path,
            worktree.branch,
            worktree.head,
            session,
        );
        let _ = agent::ensure_agent_session(&adopted);
        repo.agents.push(adopted);
        added = true;
    }
    (added, previous != child_paths(repo))
}

fn child_paths(repo: &RepoNode) -> HashMap<String, HashSet<String>> {
    repo.agents
        .iter()
        .map(|agent| {
            (
                path_key(&agent.worktree_path),
                agent
                    .children
                    .iter()
                    .map(|child| path_key(&child.path))
                    .collect(),
            )
        })
        .collect()
}

fn probe(repo: &RepoNode, parent: usize, worktree: Worktree, config: &Config) -> ChildWorktree {
    let agent = &repo.agents[parent];
    let ahead_behind = worktree
        .branch
        .as_deref()
        .and_then(|branch| git::ahead_behind(&repo.path, &agent.branch, branch).ok());
    let modified_at = std::fs::metadata(&worktree.path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .map(DateTime::<Local>::from);
    ChildWorktree {
        clean: git::tracked_tree_is_clean(&worktree.path).ok(),
        tmux_session: crate::tmux::find_session_by_cwd(&config.tmux_session_prefix, &worktree.path),
        path: worktree.path,
        branch: worktree.branch,
        head: worktree.head,
        ahead_behind,
        modified_at,
    }
}

#[cfg(test)]
#[path = "children_tests.rs"]
mod tests;
