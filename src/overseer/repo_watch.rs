//! When Overseer last ran the periodic advisory/Dependabot health watch for
//! each managed repository — see `crate::overseer::daemon::repo_watch`.
//!
//! Its own file, deliberately apart from `ledger.json` and `other_prs.json`:
//! this state only gates a daily probe's cadence, not a dispatched task's
//! phase or a repository's open pull requests. Dedup of the tasks the watch
//! files lives on the dropr board itself (`daemon::repo_watch_task`), not
//! here — a lost or stale copy of this file costs an extra day's `bun
//! audit` / `govulncheck` run at worst, never a duplicate task.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::ErrorKind,
    path::Path,
};

use chrono::{DateTime, Utc};
use nanoid::nanoid;
use serde::{Deserialize, Serialize};

use crate::Result;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RepoWatchState {
    /// Keyed by repository path, matching `OtherPrs::repos`'s convention: two
    /// checkouts of the same repository are different working copies.
    pub(crate) repos: BTreeMap<String, DateTime<Utc>>,
    /// Advisory-probe tool labels (e.g. `"govulncheck"`) already reported
    /// missing at least once. A missing tool is a host-level fact that stays
    /// true every pass until someone installs it, so the notice fires only
    /// the first time rather than on every due repository forever
    /// (dropr task #445).
    #[serde(default)]
    pub(crate) reported_missing_tools: BTreeSet<String>,
}

impl RepoWatchState {
    pub(crate) fn load() -> Result<Self> {
        Self::load_from(&super::repo_watch_path()?)
    }

    pub(crate) fn save(&self) -> Result<()> {
        self.save_to(&super::repo_watch_path()?)
    }

    fn load_from(path: &Path) -> Result<Self> {
        let raw = match fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error.into()),
        };
        // A cache, not a ledger: a corrupt file costs a re-check on the next
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
}

#[cfg(test)]
#[path = "repo_watch_tests.rs"]
mod tests;
