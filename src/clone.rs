use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

use crate::{Error, Result, git};

pub fn clone_and_register(
    url: &str,
    repos_root: &Path,
    branch: Option<&str>,
    name: Option<&str>,
) -> Result<PathBuf> {
    let path = clone_repo(url, repos_root, branch, name)?;
    crate::registry::Registry::locked_add_pinned(&path)?;
    Ok(path)
}

pub fn clone_repo(
    url: &str,
    repos_root: &Path,
    branch: Option<&str>,
    name: Option<&str>,
) -> Result<PathBuf> {
    let repo_name = match name {
        Some(name) => name.to_string(),
        None => repo_name(url).ok_or_else(|| Error::Command {
            context: "git clone",
            stderr: "could not derive repository name from URL".to_string(),
        })?,
    };
    validate_repo_name(&repo_name)?;
    let destination = repos_root.join(repo_name);
    if destination.exists() {
        return Err(Error::Command {
            context: "git clone",
            stderr: format!("destination already exists: {}", destination.display()),
        });
    }
    fs::create_dir_all(repos_root)?;

    let mut command = Command::new("git");
    command.arg("clone");
    if let Some(branch) = branch {
        command.args(["-b", branch]);
    }
    let output = command.arg(url).arg(&destination).output()?;
    git::command_unit(output, "git clone")?;
    Ok(destination.canonicalize().unwrap_or(destination))
}

pub fn looks_like_git_url(value: &str) -> bool {
    let value = value.trim();
    ["https://", "http://", "ssh://", "git://", "file://"]
        .iter()
        .any(|scheme| value.starts_with(scheme))
        || value
            .split_once(':')
            .is_some_and(|(host, path)| host.contains('@') && !path.is_empty())
}

fn repo_name(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/');
    let segment = trimmed.rsplit(['/', ':']).next()?;
    let name = segment.strip_suffix(".git").unwrap_or(segment);
    (!name.is_empty()).then(|| name.to_string())
}

/// Whether `name` is safe to use as a single directory name: no separators,
/// no `.`/`..`, not empty. Shared with `src/rename.rs`, which needs the same
/// check for a renamed repository's new directory name.
pub(crate) fn is_plain_component(name: &str) -> bool {
    let mut components = Path::new(name).components();
    !name.is_empty()
        && !name.contains(['/', '\\'])
        && matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
}

fn validate_repo_name(name: &str) -> Result<()> {
    if is_plain_component(name) {
        return Ok(());
    }
    Err(Error::Command {
        context: "git clone",
        stderr: format!("invalid repository name {name:?}: expected a single plain path component"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_names_from_common_git_urls() {
        assert_eq!(
            repo_name("https://host/owner/repo.git").as_deref(),
            Some("repo")
        );
        assert_eq!(
            repo_name("git@host:owner/repo.git").as_deref(),
            Some("repo")
        );
        assert_eq!(
            repo_name("ssh://git@host:29418/prefix/owner/repo.git").as_deref(),
            Some("repo")
        );
        assert_eq!(repo_name("file:///tmp/repo.git").as_deref(), Some("repo"));
    }

    #[test]
    fn detects_host_agnostic_git_urls() {
        for url in [
            "https://host/owner/repo.git",
            "git@host:owner/repo.git",
            "ssh://git@host:29418/prefix/owner/repo.git",
            "file:///tmp/repo.git",
            "git://host/repo.git",
        ] {
            assert!(looks_like_git_url(url), "{url}");
        }
        for path in ["./repo", "/tmp/repo", "repo", "host/repo"] {
            assert!(!looks_like_git_url(path), "{path}");
        }
    }

    #[test]
    fn explicit_name_overrides_url_name() {
        let source = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "-q", "--bare"])
            .current_dir(source.path())
            .status()
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        let cloned = clone_repo(
            &format!("file://{}", source.path().display()),
            root.path(),
            None,
            Some("custom"),
        )
        .unwrap();
        assert_eq!(cloned.file_name().unwrap(), "custom");
    }

    #[test]
    fn rejects_unsafe_explicit_names() {
        for name in ["../evil", "/tmp/evil", "nested/evil", r"nested\evil"] {
            let error = clone_repo(
                "https://host/owner/repo.git",
                Path::new("root"),
                None,
                Some(name),
            )
            .unwrap_err();
            assert!(
                error.to_string().contains("single plain path component"),
                "{name}"
            );
        }
    }

    #[test]
    fn rejects_unsafe_url_derived_names() {
        for url in [
            "https://host/.",
            "https://host/..",
            r"https://host/bad\name",
        ] {
            let error = clone_repo(url, Path::new("root"), None, None).unwrap_err();
            assert!(
                error.to_string().contains("single plain path component"),
                "{url}"
            );
        }
    }
}
