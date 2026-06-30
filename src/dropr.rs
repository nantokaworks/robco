use std::{collections::HashMap, process::Command};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DroprWorkspace {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub repo_url: String,
}

#[derive(Debug, Default, Clone)]
pub struct DroprOverlay {
    by_canonical_repo: HashMap<String, DroprWorkspace>,
}

impl DroprOverlay {
    pub fn load_best_effort() -> Self {
        let Ok(output) = Command::new("dropr").args(["workspace", "list"]).output() else {
            return Self::default();
        };

        if !output.status.success() {
            return Self::default();
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut by_canonical_repo = HashMap::new();
        for line in stdout.lines().skip(1) {
            if let Some(workspace) = parse_workspace_line(line)
                && let Some(canonical) = canonical_repo(&workspace.repo_url)
            {
                by_canonical_repo.insert(canonical, workspace);
            }
        }

        Self { by_canonical_repo }
    }

    pub fn find_by_repo_url(&self, repo_url: &str) -> Option<&DroprWorkspace> {
        canonical_repo(repo_url).and_then(|key| self.by_canonical_repo.get(&key))
    }
}

fn parse_workspace_line(line: &str) -> Option<DroprWorkspace> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let repo_start = trimmed
        .find("http://")
        .or_else(|| trimmed.find("https://"))?;
    let (left, repo_url) = trimmed.split_at(repo_start);
    let mut parts = left.split_whitespace();
    let kind = parts.next()?.to_string();
    let id = parts.next()?.to_string();
    let name = parts.collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return None;
    }

    Some(DroprWorkspace {
        kind,
        id,
        name,
        repo_url: repo_url.trim().to_string(),
    })
}

pub fn canonical_repo(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches(".git");
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        return Some(format!("github:{}", rest.to_ascii_lowercase()));
    }
    for prefix in [
        "https://github.com/",
        "http://github.com/",
        "ssh://git@github.com/",
    ] {
        if let Some(rest) = url.strip_prefix(prefix) {
            return Some(format!("github:{}", rest.to_ascii_lowercase()));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_common_github_urls() {
        assert_eq!(
            canonical_repo("https://github.com/NantokaWorks/robco.git"),
            Some("github:nantokaworks/robco".to_string())
        );
        assert_eq!(
            canonical_repo("git@github.com:nantokaworks/dropr.git"),
            Some("github:nantokaworks/dropr".to_string())
        );
    }

    #[test]
    fn parses_workspace_line() {
        let line = "  materialised  Xdin9xDHmhuOohKzCBmZX                 dropr                 https://github.com/nantokaworks/dropr.git";
        let workspace = parse_workspace_line(line).unwrap();
        assert_eq!(workspace.kind, "materialised");
        assert_eq!(workspace.name, "dropr");
        assert_eq!(
            workspace.repo_url,
            "https://github.com/nantokaworks/dropr.git"
        );
    }
}
