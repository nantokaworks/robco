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
    /// Whether an `Escalate` decision belongs to `daemon::merge_escalation`'s
    /// terminal/transient vocabulary, and if so, which half: `Some(true)` for
    /// one nothing will reconsider (notify), `Some(false)` for one the
    /// merge-hold recheck loop may still resolve on its own (suppress until
    /// it crosses the stuck threshold). `None` for every decision outside
    /// that vocabulary — those notify exactly as they did before this field
    /// existed, via `notifications::from_decision`'s pre-existing match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escalation_notify: Option<bool>,
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
            escalation_notify: None,
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
    // Serialize into a buffer first and issue exactly one `write_all` on the
    // `O_APPEND` descriptor. `O_APPEND` only makes a single `write` syscall
    // atomic; `serde_json::to_writer` straight to the file emits one syscall
    // per token, so two processes serializing at the same time interleaved at
    // token granularity and shredded both records (dropr:VYj8In1jqvunWtAy3OtCo).
    let mut buf = serde_json::to_vec(entry)?;
    buf.push(b'\n');
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(&buf)?;
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

/// Count of lines in the decision log that exist but do not parse as a
/// `DecisionEntry` — e.g. two records shredded together by a non-atomic
/// append. Every reader (`tail_from`, `DigestCursor`, the Discord cursor)
/// already skips a line it cannot parse, because there is nothing else to do
/// with it; this is the counterpart that makes the skip visible instead of
/// silent, surfaced by `robco overseer status` (dropr:VYj8In1jqvunWtAy3OtCo).
///
/// A fresh full-file scan rather than a running counter: readers only see the
/// window they poll (`tail_from`'s last 64KiB, the cursor's unread tail), so
/// a counter fed by them would undercount corruption outside that window, and
/// would need to survive daemon restarts to stay accurate. A scan is cheap at
/// the file sizes this log reaches and needs no persisted state.
pub fn corrupt_line_count() -> Result<usize> {
    corrupt_line_count_at(&super::decision_log_path()?)
}

pub(crate) fn corrupt_line_count_at(path: &Path) -> Result<usize> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    };
    let reader = BufReader::new(file);
    Ok(reader
        .lines()
        .map_while(std::result::Result::ok)
        .filter(|line| !line.trim().is_empty())
        .filter(|line| serde_json::from_str::<DecisionEntry>(line).is_err())
        .count())
}

mod compact;
pub use compact::{CompactionReport, compact};

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

pub(crate) fn tail_from(path: &Path, limit: usize) -> Result<Vec<DecisionEntry>> {
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
#[path = "logging_tests.rs"]
mod tests;
