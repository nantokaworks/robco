use std::{collections::BTreeMap, fs, path::Path};

use crate::{
    Result,
    dropr::DroprTaskFetch,
    git,
    model::{ManagementMode, RepoNode},
};

pub fn discover_repos(launch_dir: &Path) -> Result<Vec<RepoNode>> {
    let launch_dir = launch_dir.canonicalize()?;
    let mut repos = Vec::new();

    for entry in fs::read_dir(launch_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() || !is_git_repo(&path) {
            continue;
        }

        let path = path.canonicalize()?;
        repos.push(repo_node(path, false));
    }

    repos.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(repos)
}

pub fn discover_all<'a>(roots: impl IntoIterator<Item = &'a Path>) -> Vec<RepoNode> {
    let mut repos = BTreeMap::new();
    for root in roots {
        let Ok(discovered) = discover_repos(root) else {
            continue;
        };
        for repo in discovered {
            repos.entry(repo.path.clone()).or_insert(repo);
        }
    }
    let mut repos: Vec<_> = repos.into_values().collect();
    repos.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
    repos
}

pub fn discover_with_overlay(
    roots: &[std::path::PathBuf],
    config: &crate::config::Config,
    indicator: &crate::loading::Indicator,
) -> Vec<RepoNode> {
    let mut discovered = discover_all(roots.iter().map(std::path::PathBuf::as_path));
    if config.dropr_overlay {
        indicator.set_message("Loading dropr workspaces...");
        let overlay = crate::dropr::DroprOverlay::load_best_effort();
        for repo in &mut discovered {
            if let Some(remote) = &repo.remote_url {
                repo.dropr = overlay.find_by_repo_url(remote).cloned();
            }
        }
    }
    discovered
}

pub(crate) fn repo_node(path: std::path::PathBuf, pinned: bool) -> RepoNode {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("repo")
        .to_string();
    let remote_url = git::remote_url(&path).ok();
    RepoNode {
        path,
        name,
        remote_url,
        pinned,
        management: ManagementMode::Auto,
        agents: Vec::new(),
        dropr: None,
        dropr_tasks: DroprTaskFetch::default(),
        main_status: None,
        main_last_capture: None,
        main_last_spinner: None,
        main_last_change_at: None,
        main_shell_working: false,
        main_mcp_active: false,
        main_pane_pid: None,
        main_tracked_command: None,
        main_subagents_active: 0,
        main_behind_origin: None,
        checkout_state: None,
    }
}

pub fn is_git_repo(path: &Path) -> bool {
    path.join(".git").exists()
}

#[cfg(test)]
mod tests {
    use std::{fs, process::Command};

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn discovers_only_direct_git_children() {
        let dir = tempdir().unwrap();
        let repo = dir.path().join("repo");
        let nested_parent = dir.path().join("nested");
        let nested_repo = nested_parent.join("repo");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&nested_repo).unwrap();
        fs::create_dir_all(dir.path().join("plain")).unwrap();

        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .status()
            .unwrap();
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&nested_repo)
            .status()
            .unwrap();

        let repos = discover_repos(dir.path()).unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].name, "repo");
    }

    #[test]
    fn discover_all_skips_missing_roots_and_deduplicates() {
        let dir = tempdir().unwrap();
        let repo = dir.path().join("repo");
        fs::create_dir(&repo).unwrap();
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .status()
            .unwrap();
        let alias = dir.path().join("alias");
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path(), &alias).unwrap();

        let missing = dir.path().join("missing");
        #[cfg(unix)]
        let roots = [dir.path(), alias.as_path(), missing.as_path()];
        #[cfg(not(unix))]
        let roots = [dir.path(), dir.path(), missing.as_path()];
        assert_eq!(discover_all(roots).len(), 1);
    }
}
