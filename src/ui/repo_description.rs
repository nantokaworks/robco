use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
    thread,
};

use crate::model::RepoNode;

#[derive(Clone)]
enum State {
    Pending,
    Done(Option<String>),
}

static CACHE: OnceLock<Mutex<HashMap<PathBuf, State>>> = OnceLock::new();

pub(super) fn get(repo: &RepoNode) -> Option<String> {
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = cache.lock().ok()?;
    if let Some(state) = entries.get(&repo.path) {
        return match state {
            State::Pending => None,
            State::Done(description) => description.clone(),
        };
    }

    entries.insert(repo.path.clone(), State::Pending);
    let path = repo.path.clone();
    let remote = repo.remote_url.clone();
    thread::spawn(move || {
        let manifest = manifest_description(&path);
        publish(&path, manifest.clone());
        let github = remote
            .as_deref()
            .and_then(github_slug)
            .and_then(github_description);
        if github.is_some() {
            publish(&path, github);
        }
    });
    None
}

fn publish(path: &Path, description: Option<String>) {
    if let Ok(mut entries) = CACHE.get().expect("cache initialized").lock() {
        entries.insert(path.to_path_buf(), State::Done(description));
    }
}

fn github_description(slug: String) -> Option<String> {
    let output = Command::new("gh")
        .args(["repo", "view", &slug, "--json", "description"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    clean(value.get("description")?.as_str()?)
}

fn github_slug(remote: &str) -> Option<String> {
    let path = remote
        .strip_prefix("git@github.com:")
        .or_else(|| remote.strip_prefix("https://github.com/"))?;
    let slug = path.trim_end_matches('/').trim_end_matches(".git");
    let mut parts = slug.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

fn manifest_description(root: &Path) -> Option<String> {
    fs::read_to_string(root.join("Cargo.toml"))
        .ok()
        .and_then(|text| cargo_description(&text))
        .or_else(|| {
            let text = fs::read_to_string(root.join("package.json")).ok()?;
            let value: serde_json::Value = serde_json::from_str(&text).ok()?;
            clean(value.get("description")?.as_str()?)
        })
}

fn cargo_description(text: &str) -> Option<String> {
    let mut in_package = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_package = section_name(line).is_some_and(|name| name == "package");
            continue;
        }
        if in_package {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if key.trim() == "description" {
                return basic_string(value.trim());
            }
        }
    }
    None
}

fn section_name(line: &str) -> Option<&str> {
    let inner = line.strip_prefix('[')?.split_once(']')?.0;
    Some(inner.trim())
}

fn basic_string(value: &str) -> Option<String> {
    if value.starts_with("\"\"\"") || value.starts_with("'''") {
        return None;
    }
    let mut chars = value.strip_prefix('"')?.chars();
    let mut parsed = String::new();
    let mut escaped = false;
    for character in chars.by_ref() {
        if escaped {
            parsed.push(match character {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                other => other,
            });
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            return clean(&parsed);
        } else {
            parsed.push(character);
        }
    }
    None
}

fn clean(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_github_remote_forms() {
        assert_eq!(
            github_slug("git@github.com:owner/repo.git").as_deref(),
            Some("owner/repo")
        );
        assert_eq!(
            github_slug("https://github.com/owner/repo").as_deref(),
            Some("owner/repo")
        );
        assert!(github_slug("https://gitlab.com/owner/repo").is_none());
    }

    #[test]
    fn parses_only_package_description() {
        let text = "description = \"workspace\"\n[package]\nname = \"x\"\ndescription = \"A repo\"\n[dependencies]\ndescription = \"dep\"";
        assert_eq!(cargo_description(text).as_deref(), Some("A repo"));
    }

    #[test]
    fn parses_basic_string_details() {
        assert_eq!(
            cargo_description("[package]\ndescription = \"A repo\" # useful"),
            Some("A repo".into())
        );
        assert_eq!(
            cargo_description("[package]\ndescription = \"A \\\"quoted\\\" repo\""),
            Some("A \"quoted\" repo".into())
        );
        assert_eq!(
            cargo_description("[ package ]\ndescription = \"spaced\""),
            Some("spaced".into())
        );
    }

    #[test]
    fn skips_multiline_basic_string() {
        assert!(cargo_description("[package]\ndescription = \"\"\"many\nlines\"\"\"").is_none());
    }
}
