use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use serde::Deserialize;

use super::{SubagentReader, SubagentStatus, TaskSubagent};

const SESSION_RECENCY: Duration = Duration::from_secs(10 * 60);
const RUNNING_RECENCY: Duration = Duration::from_secs(60);
const DONE_RECENCY: Duration = Duration::from_secs(10 * 60);
const FUTURE_MTIME_TOLERANCE: Duration = Duration::from_secs(2);

pub struct ClaudeSubagentReader {
    base_dir: PathBuf,
}

impl ClaudeSubagentReader {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    fn active_session(&self, worktree_path: &Path, now: SystemTime) -> Option<PathBuf> {
        let project_dir = self
            .base_dir
            .join("projects")
            .join(project_slug(worktree_path));
        let entries = fs::read_dir(&project_dir).ok()?;

        entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                let is_jsonl = path.extension().is_some_and(|ext| ext == "jsonl");
                let is_direct_file = entry.file_type().ok().is_some_and(|kind| kind.is_file());
                if !is_jsonl || !is_direct_file {
                    return None;
                }
                let modified = entry.metadata().ok()?.modified().ok()?;
                is_recent(now, modified, SESSION_RECENCY).then_some((modified, path))
            })
            .max_by_key(|(modified, _)| *modified)
            .and_then(|(_, session_file)| {
                session_file
                    .file_stem()
                    .map(|session_id| project_dir.join(session_id))
            })
    }

    fn read_session(&self, session_dir: &Path, now: SystemTime) -> Vec<TaskSubagent> {
        let subagents_dir = session_dir.join("subagents");
        let Ok(entries) = fs::read_dir(subagents_dir) else {
            return Vec::new();
        };

        entries
            .filter_map(Result::ok)
            .filter_map(|entry| read_subagent(&entry.path(), now))
            .collect()
    }
}

impl Default for ClaudeSubagentReader {
    fn default() -> Self {
        let base_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".claude");
        Self::new(base_dir)
    }
}

impl SubagentReader for ClaudeSubagentReader {
    fn read(&self, worktree_path: &Path, now: SystemTime) -> Vec<TaskSubagent> {
        self.active_session(worktree_path, now)
            .map_or_else(Vec::new, |session| self.read_session(&session, now))
    }
}

pub fn project_slug(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect()
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ClaudeMeta {
    agent_type: String,
    description: String,
    tool_use_id: String,
    spawn_depth: u32,
}

fn read_subagent(meta_path: &Path, now: SystemTime) -> Option<TaskSubagent> {
    let file_name = meta_path.file_name()?.to_str()?;
    let id = file_name
        .strip_prefix("agent-")?
        .strip_suffix(".meta.json")?;
    let meta: ClaudeMeta = serde_json::from_slice(&fs::read(meta_path).ok()?).ok()?;
    let activity_path = meta_path.with_file_name(format!("agent-{id}.jsonl"));
    let metadata = fs::metadata(activity_path).ok()?;
    let last_activity_at = metadata.modified().ok()?;
    let age = timestamp_age(now, last_activity_at)?;
    let status = if age <= RUNNING_RECENCY {
        SubagentStatus::Running
    } else {
        SubagentStatus::Done
    };
    if status == SubagentStatus::Done && age > DONE_RECENCY {
        return None;
    }

    Some(TaskSubagent {
        id: id.to_string(),
        agent_type: meta.agent_type,
        description: meta.description,
        spawn_depth: meta.spawn_depth,
        started_at: metadata.created().unwrap_or(last_activity_at),
        last_activity_at,
        status,
    })
}

fn is_recent(now: SystemTime, timestamp: SystemTime, threshold: Duration) -> bool {
    timestamp_age(now, timestamp).is_some_and(|age| age <= threshold)
}

fn timestamp_age(now: SystemTime, timestamp: SystemTime) -> Option<Duration> {
    match now.duration_since(timestamp) {
        Ok(age) => Some(age),
        Err(error) if error.duration() <= FUTURE_MTIME_TOLERANCE => Some(Duration::ZERO),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests;
