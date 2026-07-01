use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use crate::{agent, config::Config, discover, dropr, git, model::RepoNode};

use super::super::App;

impl App {
    /// Re-scan the launch directory for new projects and each repo for worktrees
    /// created outside robco, merging anything new into the registry. The
    /// current selection and per-repo expand/collapse state are preserved across
    /// the refresh even when repos are re-ordered.
    pub(in crate::ui) fn refresh_discovery(&mut self) {
        let Ok(mut discovered) = discover::discover_repos(&self.launch_dir) else {
            return;
        };

        let discovered_paths: HashSet<String> =
            discovered.iter().map(|repo| path_key(&repo.path)).collect();
        let current_paths: HashSet<String> = self
            .registry
            .repos
            .iter()
            .map(|repo| path_key(&repo.path))
            .collect();
        let repos_changed = discovered_paths != current_paths;

        // Snapshot identity-keyed state so selection and expansion survive any
        // re-ordering caused by a newly-added project sorting into the middle.
        let selected_identity = self.selected_item().map(|sel| self.item_key(sel));
        let expanded_by_path: HashMap<String, bool> = self
            .registry
            .repos
            .iter()
            .zip(self.expanded.iter())
            .map(|(repo, expanded)| (path_key(&repo.path), *expanded))
            .collect();

        if repos_changed {
            if self.config.dropr_overlay {
                let overlay = dropr::DroprOverlay::load_best_effort();
                for repo in &mut discovered {
                    if let Some(remote) = &repo.remote_url {
                        repo.dropr = overlay.find_by_repo_url(remote).cloned();
                    }
                }
            }
            self.registry.merge_discovered(discovered);
            self.expanded = self
                .registry
                .repos
                .iter()
                .map(|repo| {
                    expanded_by_path
                        .get(&path_key(&repo.path))
                        .copied()
                        .unwrap_or(true)
                })
                .collect();
        }

        let mut worktrees_added = false;
        for repo in &mut self.registry.repos {
            worktrees_added |= adopt_external_worktrees(repo, &self.config);
        }

        if repos_changed || worktrees_added {
            let _ = self.registry.save();
            self.restore_selection(selected_identity);
        }
    }

    /// Re-point the selection at the item it referred to before a refresh,
    /// falling back to a clamp when that item no longer exists.
    fn restore_selection(&mut self, identity: Option<String>) {
        if let Some(identity) = identity
            && let Some(index) = self
                .visible()
                .into_iter()
                .position(|sel| self.item_key(sel) == identity)
        {
            self.selected = index;
        }
        self.clamp_selection();
    }
}

/// Add an agent for any worktree of `repo` that exists on disk but is not yet
/// tracked. The main worktree and already-known agents are skipped. Returns
/// whether any agent was added.
fn adopt_external_worktrees(repo: &mut RepoNode, config: &Config) -> bool {
    let Ok(worktrees) = git::list_worktrees(&repo.path) else {
        return false;
    };

    let mut known: HashSet<String> = repo
        .agents
        .iter()
        .map(|agent| path_key(&agent.worktree_path))
        .collect();
    known.insert(path_key(&repo.path));

    let mut added = false;
    for worktree in worktrees {
        if !known.insert(path_key(&worktree.path)) {
            continue;
        }
        let adopted =
            agent::adopt_worktree(repo, config, worktree.path, worktree.branch, worktree.head);
        // Auto-open the AI for a worktree the first time it is discovered, so a
        // worktree is "open" by default (matching robco-created worktrees, which
        // launch on creation). This fires once per newly-adopted worktree — the
        // `known` guard above means an already-tracked worktree is never
        // re-adopted, so a session the user deliberately closed is NOT
        // relaunched on the next discovery tick; Enter re-opens it in that case.
        let _ = agent::ensure_agent_session(&adopted);
        repo.agents.push(adopted);
        added = true;
    }
    added
}

/// Canonical string key for a path, used to compare worktree paths that git and
/// robco may spell differently (symlinks, trailing components). Falls back to
/// the lexical path when the path cannot be canonicalized.
fn path_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}
