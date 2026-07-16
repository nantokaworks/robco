use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path, PathBuf},
};

use crate::{discover, dropr, git};

use super::super::App;

impl App {
    /// Re-scan the launch directory for new projects and each repo for worktrees
    /// created outside robco, merging anything new into the registry. The
    /// current selection and per-repo expand/collapse state are preserved across
    /// the refresh even when repos are re-ordered.
    pub(in crate::ui) fn refresh_discovery(&mut self) {
        if !self.config.subagent_indicator {
            self.refresh_subagents();
        }
        let Ok(mut discovered) = discover::discover_repos(&self.launch_dir) else {
            return;
        };

        let selected_identity = self.selected_item().map(|sel| self.item_key(sel));

        let worktrees_removed = self.prune_unmanaged_agents();

        let mut expected_paths: HashSet<String> =
            discovered.iter().map(|repo| path_key(&repo.path)).collect();
        for repo in &self.registry.repos {
            if !repo.agents.is_empty() || repo.pinned {
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

        // Preserve expansion state when newly discovered repos change the order.
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
        let mut children_changed = false;
        for repo in &mut self.registry.repos {
            if let Ok(worktrees) = git::list_worktrees(&repo.path) {
                let (added, changed) = super::children::reconcile(repo, &self.config, worktrees);
                worktrees_added |= added;
                children_changed |= changed;
            }
        }

        if repos_changed || worktrees_added || worktrees_removed {
            let _ = self.registry.save();
        }
        if repos_changed || worktrees_added || worktrees_removed || children_changed {
            self.restore_selection(selected_identity);
        }
        if self.config.subagent_indicator {
            self.refresh_subagents();
        }
        self.refresh_dropr_tasks(false);
        self.refresh_orphans();
    }

    pub(in crate::ui) fn prune_unmanaged_agents(&mut self) -> bool {
        let mut removed = false;
        for repo in &mut self.registry.repos {
            let previous_len = repo.agents.len();
            repo.agents.retain(|tracked| {
                is_managed_worktree(&tracked.worktree_path, &self.config.worktree_root)
            });
            super::slots::prune_slot_agents(repo);
            prune_nested_agents(repo);
            removed |= repo.agents.len() != previous_len;
        }
        removed
    }

    /// Re-point selection at the same item, falling back to a clamp.
    pub(in crate::ui) fn restore_selection(&mut self, identity: Option<String>) {
        if let Some(identity) = identity
            && let Some(index) = self
                .visible()
                .into_iter()
                .position(|sel| self.item_key(sel) == identity)
        {
            self.selected = index;
        }
        self.clamp_selection();
        self.restore_preview();
    }
}

fn prune_nested_agents(repo: &mut crate::model::RepoNode) {
    let paths: Vec<_> = repo
        .agents
        .iter()
        .map(|agent| agent.worktree_path.clone())
        .collect();
    repo.agents.retain(|tracked| {
        !paths.iter().any(|parent| {
            parent != &tracked.worktree_path
                && path_is_strictly_inside(&tracked.worktree_path, parent)
        })
    });
}

/// Canonical string key for a path, used to compare worktree paths that git and
/// robco may spell differently (symlinks, trailing components). Falls back to
/// the lexical path when the path cannot be canonicalized.
pub(super) fn path_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

pub(super) fn path_is_strictly_inside(path: &Path, parent: &Path) -> bool {
    let path = path.canonicalize().unwrap_or_else(|_| normalize_path(path));
    let parent = parent
        .canonicalize()
        .unwrap_or_else(|_| normalize_path(parent));
    path != parent && path.starts_with(parent)
}

pub(super) fn is_managed_worktree(path: &Path, worktree_root: &Path) -> bool {
    let canonical_path = path.canonicalize();
    let canonical_root = worktree_root.canonicalize();

    if let (Ok(path), Ok(root)) = (&canonical_path, &canonical_root) {
        return path.starts_with(root);
    }

    let path = normalize_path(path);
    path.starts_with(normalize_path(worktree_root))
        || canonical_root.is_ok_and(|root| path.starts_with(root))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component);
            }
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::{is_managed_worktree, prune_nested_agents};
    use crate::{agent, config::Config, model::RepoNode};
    use std::path::Path;

    #[test]
    fn managed_path_must_be_beneath_root() {
        assert!(is_managed_worktree(
            Path::new("/tmp/robco/worktrees/task"),
            Path::new("/tmp/robco/worktrees")
        ));
        assert!(!is_managed_worktree(
            Path::new("/tmp/robco/worktrees-other/task"),
            Path::new("/tmp/robco/worktrees")
        ));
    }

    #[cfg(unix)]
    #[test]
    fn managed_path_resolves_symlinked_root() {
        use std::{fs, os::unix::fs::symlink};

        let temp = tempfile::tempdir().unwrap();
        let real_root = temp.path().join("real");
        let linked_root = temp.path().join("linked");
        let worktree = real_root.join("task");
        fs::create_dir_all(&worktree).unwrap();
        symlink(&real_root, &linked_root).unwrap();

        assert!(is_managed_worktree(&worktree, &linked_root));
    }

    #[cfg(unix)]
    #[test]
    fn missing_managed_path_keeps_symlink_spelling() {
        use std::{fs, os::unix::fs::symlink};

        let temp = tempfile::tempdir().unwrap();
        let real_root = temp.path().join("real");
        let linked_root = temp.path().join("linked");
        fs::create_dir_all(&real_root).unwrap();
        symlink(&real_root, &linked_root).unwrap();

        let missing_worktree = linked_root.join("missing-task");
        assert!(!missing_worktree.exists());
        assert!(is_managed_worktree(&missing_worktree, &linked_root));
    }

    #[test]
    fn existing_path_cannot_escape_root_with_parent_component() {
        use std::fs;

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let outside = temp.path().join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();

        assert!(!is_managed_worktree(&root.join("../outside"), &root));
    }

    #[test]
    fn missing_path_cannot_escape_root_with_parent_component() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        assert!(!is_managed_worktree(
            &root.join("../missing-outside"),
            &root
        ));
    }

    #[test]
    fn legacy_nested_agents_are_pruned_without_prefix_false_positives() {
        let config = Config::default();
        let mut repo = RepoNode {
            path: "/repo".into(),
            name: "repo".into(),
            remote_url: None,
            pinned: false,
            agents: Vec::new(),
            dropr: None,
            dropr_tasks: Vec::new(),
            main_status: None,
            main_last_capture: None,
            main_last_change_at: None,
            main_shell_working: false,
            main_pane_pid: None,
            main_tracked_command: None,
            main_subagents_active: 0,
        };
        for path in ["/wt/foo", "/wt/foo/nested", "/wt/foo-bar"] {
            repo.agents.push(agent::adopt_worktree(
                &repo,
                &config,
                path.into(),
                None,
                None,
                None,
                None,
            ));
        }
        prune_nested_agents(&mut repo);

        let paths: Vec<_> = repo
            .agents
            .iter()
            .map(|agent| agent.worktree_path.as_path())
            .collect();
        assert_eq!(paths, [Path::new("/wt/foo"), Path::new("/wt/foo-bar")]);
    }
}
