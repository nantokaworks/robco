use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::statefile::{atomic_replace, with_lock};
use crate::Result;

const ROTATION_THRESHOLD: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InboxReport {
    pub at: DateTime<Utc>,
    pub agent_id: String,
    pub kind: ReportKind,
}

impl InboxReport {
    pub fn lifecycle(agent_id: String, kind: &str) -> Option<Self> {
        Some(Self {
            at: Utc::now(),
            agent_id,
            kind: ReportKind::parse(kind)?,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReportKind {
    Claimed,
    Done,
    Blocked,
    TurnDone,
    Waiting,
    /// The task is ordered behind another dropr task that has not merged
    /// yet — distinct from `Blocked`, which `monitor::apply::apply_inbox`
    /// escalates to an operator. A worker reports this instead once it has
    /// created the `blocks` dependency edge itself (dropr:375); it never
    /// escalates on its own, only after `overseer.max_prerequisite_wait_hours`.
    WaitingPrerequisite,
    /// A human answered the worker directly inside its own session rather
    /// than through dropr or the Inbox — or the worker's session merely
    /// looked dead to a stale probe. `monitor::apply::apply_inbox` revives
    /// the ledger entry immediately on receiving this, the same way it does
    /// for `blocked`'s escalation, instead of waiting for the daemon's own
    /// next observation pass to notice the tmux activity on its own, or for
    /// an operator to ask for a merge by hand (dropr:575).
    Unblocked,
}

impl ReportKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "claimed" => Some(Self::Claimed),
            "done" => Some(Self::Done),
            "blocked" => Some(Self::Blocked),
            "turn-done" => Some(Self::TurnDone),
            "waiting" => Some(Self::Waiting),
            "waiting-prerequisite" => Some(Self::WaitingPrerequisite),
            "unblocked" => Some(Self::Unblocked),
            _ => None,
        }
    }
}

pub fn append_report(report: &InboxReport) -> Result<()> {
    append_report_to(&super::inbox_path()?, report)
}

pub fn append_report_to(path: &Path, report: &InboxReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    with_lock(path, || {
        let mut file = OpenOptions::new().append(true).create(true).open(path)?;
        let mut line = serde_json::to_vec(report)?;
        line.push(b'\n');
        file.write_all(&line)?;
        file.flush()?;
        Ok(())
    })
}

pub struct InboxReader {
    path: PathBuf,
    offset_path: PathBuf,
    offset: u64,
    pending_offset: Option<u64>,
    rotation_threshold: u64,
}

impl InboxReader {
    pub fn new() -> Result<Self> {
        Self::from_path(super::inbox_path()?)
    }

    pub fn from_path(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let offset_path = path.with_extension("offset");
        let offset = load_offset(&offset_path)?;
        Ok(Self {
            path,
            offset_path,
            offset,
            pending_offset: None,
            rotation_threshold: ROTATION_THRESHOLD,
        })
    }

    pub fn read_new(&mut self) -> Result<Vec<InboxReport>> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let path = self.path.clone();
        with_lock(&path, || self.read_locked())
    }

    pub fn commit(&mut self) -> Result<()> {
        let Some(offset) = self.pending_offset else {
            return Ok(());
        };
        let path = self.path.clone();
        with_lock(&path, || {
            self.offset = offset;
            if self.offset >= self.rotation_threshold && self.path.exists() {
                self.rotate_locked()?;
            } else {
                persist_offset(&self.offset_path, self.offset)?;
            }
            self.pending_offset = None;
            Ok(())
        })
    }

    fn read_locked(&mut self) -> Result<Vec<InboxReport>> {
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.pending_offset = Some(0);
                return Ok(Vec::new());
            }
            Err(error) => return Err(error.into()),
        };
        let length = file.metadata()?.len();
        let mut offset = if self.offset > length { 0 } else { self.offset };
        let mut reader = BufReader::new(file);
        reader.seek(SeekFrom::Start(offset))?;
        let mut reports = Vec::new();
        let mut line = Vec::new();

        loop {
            line.clear();
            let line_start = offset;
            let read = reader.read_until(b'\n', &mut line)?;
            if read == 0 || !line.ends_with(b"\n") {
                offset = line_start;
                break;
            }
            offset += read as u64;
            match serde_json::from_slice::<InboxReport>(&line) {
                Ok(report) => reports.push(report),
                Err(error) => eprintln!(
                    "warning: skipping malformed overseer inbox line at byte {line_start}: {error}"
                ),
            }
        }

        self.pending_offset = Some(offset);
        Ok(reports)
    }

    fn rotate_locked(&mut self) -> Result<()> {
        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(self.offset))?;
        let mut tail = Vec::new();
        file.read_to_end(&mut tail)?;

        // Keep the old offset larger than the replacement file. If interrupted
        // after this rename, startup detects the mismatch and re-reads only tail.
        if tail.len() as u64 >= self.offset {
            persist_offset(&self.offset_path, self.offset)?;
            return Ok(());
        }
        atomic_replace(&self.path, &tail)?;
        persist_offset(&self.offset_path, 0)?;
        self.offset = 0;
        Ok(())
    }
}

fn load_offset(path: &Path) -> Result<u64> {
    match fs::read_to_string(path) {
        Ok(raw) => Ok(raw.trim().parse().unwrap_or_else(|error| {
            eprintln!("warning: invalid overseer inbox offset: {error}");
            0
        })),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error.into()),
    }
}

fn persist_offset(path: &Path, offset: u64) -> Result<()> {
    atomic_replace(path, offset.to_string().as_bytes())
}

#[cfg(test)]
#[path = "inbox_tests.rs"]
mod tests;
