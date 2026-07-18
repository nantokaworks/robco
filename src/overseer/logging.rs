use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Seek, SeekFrom, Write},
    path::Path,
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
}
