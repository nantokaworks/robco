use std::{fs, path::Path};

use crate::{Result, git, model::RepoNode};

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
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("repo")
            .to_string();
        let remote_url = git::remote_url(&path).ok();
        repos.push(RepoNode {
            path,
            name,
            remote_url,
            agents: Vec::new(),
            dropr: None,
            main_status: None,
            main_last_capture: None,
            main_last_change_at: None,
            main_shell_working: false,
            main_pane_pid: None,
            main_tracked_command: None,
            main_subagents_active: 0,
        });
    }

    repos.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(repos)
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
}
