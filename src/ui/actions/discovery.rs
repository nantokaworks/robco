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

        // What the registry will hold after a merge: the discovered set plus
        // any repo that keeps its registration through `merge_discovered`
        // because it still tracks agents (e.g. registered from another launch
        // directory). Comparing against this — not the raw discovered set —
        // keeps the steady state a no-op instead of re-merging and re-saving
        // on every discovery tick.
        let mut expected_paths: HashSet<String> =
            discovered.iter().map(|repo| path_key(&repo.path)).collect();
        for repo in &self.registry.repos {
            if !repo.agents.is_empty() {
                expected_paths.insert(path_key(&repo.path));
            }
        }
        let current_paths: HashSet<String> = self
            .registry
            .repos
            .iter()
            .map(|repo| path_key(&repo.path))
            .collect();
        let repos_changed = expected_paths != current_paths;

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

        self.refresh_orphans();
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
        // Prefer a live AI session already running in this worktree (matched by
        // cwd) so adoption reattaches to it even when its name predates the
        // current naming scheme, instead of spawning a duplicate below.
        let existing_session =
            crate::tmux::find_session_by_cwd(&config.tmux_session_prefix, &worktree.path);
        let adopted = agent::adopt_worktree(
            repo,
            config,
            worktree.path,
            worktree.branch,
            worktree.head,
            existing_session,
        );
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
