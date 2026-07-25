use std::path::{Component, Path, PathBuf};

use super::super::App;

impl App {
    pub(in crate::ui) fn prune_unmanaged_agents(&mut self) -> bool {
        prune_unmanaged(&mut self.registry.repos, &self.config.worktree_root)
    }

    /// Re-point selection at the same item, falling back to a clamp.
    pub(in crate::ui) fn restore_selection(&mut self, identity: Option<String>) {
        self.prune_expanded_children();
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

pub(in crate::ui) fn prune_unmanaged(
    repos: &mut [crate::model::RepoNode],
    worktree_root: &Path,
) -> bool {
    let mut removed = false;
    for repo in repos {
        let previous_len = repo.agents.len();
        repo.agents
            .retain(|tracked| is_managed_worktree(&tracked.worktree_path, worktree_root));
        // Only top-level adoptions are pruned. Reconciled slots live in
        // `AgentNode::children`, so this cannot remove legitimate nested slots.
        super::slots::prune_top_level_slot_agents(repo);
        prune_nested_agents(repo);
        removed |= repo.agents.len() != previous_len;
    }
    removed
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
pub(in crate::ui) fn path_key(path: &Path) -> String {
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
            main_last_spinner: None,
            main_last_change_at: None,
            main_shell_working: false,
            main_mcp_active: false,
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
