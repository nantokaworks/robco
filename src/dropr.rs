use std::{collections::HashMap, process::Command};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DroprTaskCandidate {
    #[serde(alias = "global_display_id")]
    pub display_id: String,
    pub title: String,
    #[serde(default)]
    pub priority: String,
    #[serde(default)]
    pub status: String,
}

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

pub fn fetch_ready_tasks(workspace_id: &str) -> Option<Vec<DroprTaskCandidate>> {
    let output = Command::new("dropr")
        .args([
            "task",
            "ready",
            "--workspace",
            workspace_id,
            "--limit",
            "3",
            "--json",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_ready_tasks(&output.stdout)
}

fn parse_ready_tasks(raw: &[u8]) -> Option<Vec<DroprTaskCandidate>> {
    let value: serde_json::Value = serde_json::from_slice(raw).ok()?;
    let tasks = match value {
        serde_json::Value::Array(tasks) => tasks,
        serde_json::Value::Object(mut object) => object.remove("tasks")?.as_array()?.clone(),
        _ => return None,
    };
    tasks
        .into_iter()
        .filter_map(|task| serde_json::from_value(task).ok())
        .take(3)
        .collect::<Vec<_>>()
        .into()
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

    #[test]
    fn parses_ready_tasks_array() {
        let tasks = parse_ready_tasks(
            br##"[{"display_id":"#42","title":"Ship it","priority":"high","status":"ready"}]"##,
        )
        .unwrap();
        assert_eq!(tasks[0].display_id, "#42");
        assert_eq!(tasks[0].title, "Ship it");
        assert_eq!(tasks[0].priority, "high");
        assert_eq!(tasks[0].status, "ready");
    }

    #[test]
    fn parses_ready_tasks_object_and_global_id() {
        let tasks = parse_ready_tasks(
            br##"{"tasks":[{"global_display_id":"#7","title":"Polish UI","priority":"medium","status":"ready"}]}"##,
        )
        .unwrap();
        assert_eq!(tasks[0].display_id, "#7");
    }

    #[test]
    fn skips_malformed_ready_tasks() {
        let tasks = parse_ready_tasks(
            br##"[
                {"display_id":"#1","title":"First"},
                {"display_id":"#2"},
                {"global_display_id":"#3","title":"Third"}
            ]"##,
        )
        .unwrap();
        assert_eq!(
            tasks
                .iter()
                .map(|task| task.display_id.as_str())
                .collect::<Vec<_>>(),
            ["#1", "#3"]
        );
    }

    #[test]
    fn accepts_ready_tasks_without_priority_or_status() {
        let tasks = parse_ready_tasks(br##"[{"display_id":"#42","title":"Ship it"}]"##).unwrap();
        assert_eq!(tasks[0].display_id, "#42");
        assert_eq!(tasks[0].priority, "");
        assert_eq!(tasks[0].status, "");
    }

    #[test]
    fn rejects_malformed_ready_tasks() {
        assert!(parse_ready_tasks(b"not json").is_none());
        assert!(parse_ready_tasks(br#"{"items":[]}"#).is_none());
    }
}
