use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::Result;

const TAIL_WINDOW_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionKind {
    Dispatch,
    Skip,
    Merge,
    Hold,
    Escalate,
    CircuitOpen,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecisionEntry {
    pub at: DateTime<Utc>,
    pub kind: DecisionKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    /// Auto-merge branch-protection strictness in force when the decision was taken.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protection_mode: Option<String>,
}

impl DecisionEntry {
    pub fn new(kind: DecisionKind, reason: impl Into<String>) -> Self {
        Self {
            at: Utc::now(),
            kind,
            task: None,
            repo: None,
            reason: reason.into(),
            source: None,
            user_id: None,
            pr_url: None,
            protection_mode: None,
        }
    }
}

pub fn append(entry: &DecisionEntry) -> Result<()> {
    append_to(&super::decision_log_path()?, entry)
}

pub(crate) fn append_to(path: &Path, entry: &DecisionEntry) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut file, entry)?;
    file.write_all(b"\n")?;
    Ok(())
}

pub fn log_message(task: Option<&str>, reason: &str) -> Result<()> {
    let mut entry = DecisionEntry::new(DecisionKind::Hold, reason);
    entry.task = task.map(str::to_owned);
    entry.source = Some("daemon".into());
    append(&entry)
}

pub fn tail(limit: usize) -> Result<Vec<DecisionEntry>> {
    let path = super::decision_log_path()?;
    tail_from(&path, limit)
}

pub(crate) struct DigestCursor {
    path: PathBuf,
    offset: u64,
}

impl DigestCursor {
    pub(crate) fn at_end() -> Result<Self> {
        Self::at_end_of(super::decision_log_path()?)
    }

    fn at_end_of(path: PathBuf) -> Result<Self> {
        let offset = match fs::metadata(&path) {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error.into()),
        };
        Ok(Self { path, offset })
    }

    pub(crate) fn read_digest(&mut self) -> Result<Option<String>> {
        let mut file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if file.metadata()?.len() < self.offset {
            return Ok(None);
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let mut reader = BufReader::new(file);
        let mut entries = Vec::new();
        loop {
            let mut line = String::new();
            let bytes = reader.read_line(&mut line)?;
            if bytes == 0 || !line.ends_with('\n') {
                break;
            }
            self.offset += bytes as u64;
            if let Ok(entry) = serde_json::from_str(&line) {
                entries.push(entry);
            }
        }
        Ok(coalesce_digest(&entries))
    }
}

pub fn coalesce_digest(entries: &[DecisionEntry]) -> Option<String> {
    let alerts = entries
        .iter()
        .filter(|entry| {
            matches!(
                entry.kind,
                DecisionKind::Escalate | DecisionKind::CircuitOpen
            )
        })
        .collect::<Vec<_>>();
    if alerts.is_empty() {
        return None;
    }
    let brief = alerts
        .iter()
        .take(3)
        .map(|entry| {
            let target = entry.task.as_deref().unwrap_or("overseer");
            format!("{target}: {}", entry.reason)
        })
        .collect::<Vec<_>>()
        .join("; ");
    let remaining = alerts.len().saturating_sub(3);
    Some(if remaining == 0 {
        format!("{} overseer alert(s): {brief}", alerts.len())
    } else {
        format!(
            "{} overseer alert(s): {brief}; +{remaining} more",
            alerts.len()
        )
    })
}

fn tail_from(path: &Path, limit: usize) -> Result<Vec<DecisionEntry>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let start = file.metadata()?.len().saturating_sub(TAIL_WINDOW_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut reader = BufReader::new(file);
    if start != 0 {
        let mut partial = Vec::new();
        reader.read_until(b'\n', &mut partial)?;
    }
    let mut entries = VecDeque::with_capacity(limit);
    for entry in reader
        .lines()
        .map_while(std::result::Result::ok)
        .filter_map(|line| serde_json::from_str(&line).ok())
    {
        if entries.len() == limit {
            entries.pop_front();
        }
        if limit != 0 {
            entries.push_back(entry);
        }
    }
    Ok(entries.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_tail_skips_malformed_lines() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("decisions.jsonl");
        for index in 0..3 {
            append_to(
                &path,
                &DecisionEntry::new(DecisionKind::Skip, index.to_string()),
            )
            .unwrap();
        }
        let entries = tail_from(&path, 2).unwrap();
        assert_eq!(entries[0].reason, "1");
    }

    #[test]
    fn bounded_tail_reads_end_of_large_log() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("decisions.jsonl");
        let mut file = File::create(&path).unwrap();
        for _ in 0..(TAIL_WINDOW_BYTES / 8 + 1) {
            file.write_all(b"invalid\n").unwrap();
        }
        drop(file);
        for index in 0..3 {
            append_to(
                &path,
                &DecisionEntry::new(DecisionKind::Skip, index.to_string()),
            )
            .unwrap();
        }

        let entries = tail_from(&path, 2).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].reason, "1");
        assert_eq!(entries[1].reason, "2");
    }

    #[test]
    fn escalation_batch_produces_one_digest_not_one_line_each() {
        let entries = (0..4)
            .map(|index| {
                let mut entry =
                    DecisionEntry::new(DecisionKind::Escalate, format!("reason-{index}"));
                entry.task = Some(format!("task-{index}"));
                entry
            })
            .collect::<Vec<_>>();
        let digest = coalesce_digest(&entries).unwrap();
        assert!(digest.starts_with("4 overseer alert(s):"));
        assert_eq!(digest.lines().count(), 1);
        assert!(digest.contains("+1 more"));
    }

    #[test]
    fn offset_cursor_digests_large_gap_once() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("decisions.jsonl");
        let mut cursor = DigestCursor::at_end_of(path.clone()).unwrap();
        let entry = DecisionEntry::new(DecisionKind::Escalate, "same alert");
        for _ in 0..250 {
            append_to(&path, &entry).unwrap();
        }

        let digest = cursor.read_digest().unwrap().unwrap();
        assert!(digest.starts_with("250 overseer alert(s):"));
        assert!(cursor.read_digest().unwrap().is_none());
    }
}
