use std::{
    collections::{HashMap, HashSet},
    path::Path,
    time::Duration,
};

use chrono::{DateTime, Local};

use crate::{
    agent,
    config::{Config, ENV_AGENT_ID, ENV_PARENT_AGENT_ID},
    git::{self, Worktree},
    model::{AgentNode, ChildWorktree, RepoNode},
};

use super::discovery::{is_managed_worktree, path_is_strictly_inside, path_key};

const ADOPTION_GRACE_PERIOD: Duration = Duration::from_secs(15);

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
        // Approach (a): sibling slots reuse the existing ChildWorktree path.
        if let Some(owner) =
            super::slots::slot_owner(&worktree.path, worktree.branch.as_deref(), &repo.agents)
        {
            let child = probe(repo, owner, worktree, config);
            repo.agents[owner].children.push(child);
            continue;
        }
        // A producer slot branch without a resolvable owner must never become
        // a top-level agent; normal producer paths resolve through the directory.
        if super::slots::is_slot_worktree(&worktree.path, worktree.branch.as_deref(), &repo.agents)
        {
            continue;
        }
        if !is_managed_worktree(&worktree.path, &config.worktree_root) {
            continue;
        }
        // A worker's tmux session comes up before `spawn::persist_child`
        // writes the registry row (dropr:566), so a young worktree can
        // already have a live session by the time this refresh runs.
        // Gating on age alone — not `session.is_none()` — is what actually
        // holds back a launch still in flight; a genuine orphan session
        // always sits on an old worktree, so it is still adopted below.
        if worktree_age(&worktree.path).is_some_and(should_skip_adoption) {
            continue;
        }
        let session = crate::tmux::find_session_by_cwd(
            &config.tmux_server,
            &config.tmux_session_prefix,
            &worktree.path,
        );
        known.insert(path_key(&worktree.path));
        let recovered_id = session
            .as_deref()
            .and_then(|name| crate::tmux::session_env(&config.tmux_server, name, ENV_AGENT_ID));
        let recovered_parent = session.as_deref().and_then(|name| {
            crate::tmux::session_env(&config.tmux_server, name, ENV_PARENT_AGENT_ID)
        });
        let recovered_identity = recovered_id.map(|id| agent::RecoveredIdentity {
            id,
            parent_agent_id: recovered_parent,
        });
        added |= adopt_top_level(repo, config, worktree, session, recovered_identity);
    }
    (added, previous != child_paths(repo))
}

/// Adopt `worktree` as a new top-level agent unless the identity its session
/// advertises already belongs to a tracked row. Returns whether a row was added.
///
/// `known` rules out paths only, and `path_key` cannot recognise a worktree the
/// registry already tracks once canonicalization fails for it: a renamed or
/// removed directory falls back to its lexical spelling, which need not match
/// how git spells the same worktree. The session at the candidate path then
/// hands back an id the registry already holds, and adopting it would leave two
/// rows sharing one id. Every per-agent path (`overseer::exec`,
/// background-refresh merge-back) resolves an id to a single row, so the extra
/// row would go stale and never receive an update the tracked row got. Keeping
/// the tracked row instead loses nothing: it carries the same identity and the
/// same tmux session.
fn adopt_top_level(
    repo: &mut RepoNode,
    config: &Config,
    worktree: Worktree,
    session: Option<String>,
    recovered_identity: Option<agent::RecoveredIdentity>,
) -> bool {
    if let Some(identity) = &recovered_identity
        && repo.agents.iter().any(|agent| agent.id == identity.id)
    {
        return false;
    }
    let adopted = agent::adopt_worktree(
        repo,
        config,
        worktree.path,
        worktree.branch,
        worktree.head,
        session,
        recovered_identity,
    );
    let _ = agent::ensure_agent_session(&adopted);
    repo.agents.push(adopted);
    true
}

fn worktree_age(path: &Path) -> Option<Duration> {
    let metadata = std::fs::metadata(path).ok()?;
    let timestamp = metadata.created().or_else(|_| metadata.modified()).ok()?;
    timestamp.elapsed().ok()
}

fn should_skip_adoption(age: Duration) -> bool {
    age < ADOPTION_GRACE_PERIOD
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
        tmux_session: crate::tmux::find_session_by_cwd(
            &config.tmux_server,
            &config.tmux_session_prefix,
            &worktree.path,
        ),
        path: worktree.path,
        branch: worktree.branch,
        head: worktree.head,
        ahead_behind,
        modified_at,
    }
}

pub(in crate::ui) fn child_is_visible(owner: &AgentNode, child: &ChildWorktree) -> bool {
    let is_slot = super::slots::slot_owner(
        &child.path,
        child.branch.as_deref(),
        std::slice::from_ref(owner),
    )
    .is_some();
    // Worktree removal is authoritative. An owner that advanced past a
    // zero-ahead slot is only a best-effort merged hide; (0, 0) stays visible
    // because it can be an active slot created at the owner's current HEAD.
    !(is_slot
        && child
            .ahead_behind
            .is_some_and(|(owner_only, slot_only)| owner_only > 0 && slot_only == 0))
}

#[cfg(test)]
#[path = "children_test_support.rs"]
mod test_support;

#[cfg(test)]
#[path = "children_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "children_adoption_tests.rs"]
mod adoption_tests;
