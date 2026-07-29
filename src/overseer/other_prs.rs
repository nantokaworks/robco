//! Pull requests a watched repository has open that Overseer did not
//! dispatch — a Dependabot bump, a human's own branch, another agent's. Kept
//! apart from [`crate::overseer::ledger::Ledger`], which records only what
//! Overseer itself dispatched: folding these in would corrupt the one record
//! that answers "what did we do" (dropr task #350).
//!
//! Populated by `crate::overseer::daemon::external_prs`, read by the TUI's
//! repository INFO pane.

use std::{collections::BTreeMap, fs, io::ErrorKind, path::Path};

use chrono::{DateTime, Utc};
use nanoid::nanoid;
use serde::{Deserialize, Serialize};

use crate::Result;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OtherPr {
    pub number: u64,
    pub title: String,
    pub author: String,
    pub url: String,
    pub head_ref_name: String,
    /// GitHub's raw `mergeStateStatus` (`CLEAN`, `UNSTABLE`, `DIRTY`,
    /// `BLOCKED`, …), kept as-is so an operator triaging sees the same word
    /// GitHub's own UI shows.
    pub mergeable_state: String,
    /// The dropr display id (`#N`) this pull request's body names in a
    /// `Close Dropr: #N` directive — the same phrase every worker is
    /// instructed to write (`templates::worker_prompt`) — parsed once at
    /// refresh time so dispatch can tell a task is already covered without
    /// re-reading GitHub itself (dropr task #354).
    pub closes_task: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoOtherPrs {
    /// When this repository's open pull requests were last listed. The probe
    /// backs off against this timestamp instead of re-listing every daemon
    /// tick — see `external_prs::REFRESH_INTERVAL`.
    pub polled_at: DateTime<Utc>,
    pub prs: Vec<OtherPr>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OtherPrs {
    /// Keyed by repository path (matching `LedgerEntry::repo`'s convention),
    /// not by name: two checkouts of the same repository are different
    /// working copies.
    pub repos: BTreeMap<String, RepoOtherPrs>,
}

impl OtherPrs {
    pub fn load() -> Result<Self> {
        let path = super::other_prs_path()?;
        Self::load_from(&path)
    }

    pub fn save(&self) -> Result<()> {
        let path = super::other_prs_path()?;
        self.save_to(&path)
    }

    fn load_from(path: &Path) -> Result<Self> {
        let raw = match fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error.into()),
        };
        // A cache, not a ledger: a corrupt file costs a re-list on the next
        // due repository rather than a moved-aside file and a warning.
        Ok(serde_json::from_str(&raw).unwrap_or_default())
    }

    fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_string_pretty(self)?;
        let temp_path = path.with_extension(format!("json.{}.tmp", nanoid!()));
        let written = fs::write(&temp_path, raw).and_then(|()| fs::rename(&temp_path, path));
        if let Err(error) = written {
            let _ = fs::remove_file(temp_path);
            return Err(error.into());
        }
        Ok(())
    }

    /// The pull request in `repo`, if any, whose body already names
    /// `display_id` in a `Close Dropr:` directive — dispatch's answer to "is
    /// this task already covered", the same question task #350 answers for
    /// display.
    pub fn closing(&self, repo: &str, display_id: &str) -> Option<&OtherPr> {
        self.repos
            .get(repo)?
            .prs
            .iter()
            .find(|pr| pr.closes_task.as_deref() == Some(display_id))
    }
}

#[cfg(test)]
#[path = "other_prs_tests.rs"]
mod tests;
