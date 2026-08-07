//! How long the auto-merge pass took, and which repository was slowest.
//!
//! Evaluating repositories concurrently (`merge_concurrency`) removes the
//! easiest way an operator used to notice a slow pass — every repository's
//! transitions stretching out together — so this exists to keep a slow pass
//! visible on its own. Written every pass `auto_merge_pass` actually runs,
//! through a temp file and rename like every other state file the daemon
//! owns (see `heartbeat`), so a reader never catches a half-written record.

use std::{fs, path::Path, time::Duration};

use chrono::{DateTime, Utc};
use nanoid::nanoid;
use serde::{Deserialize, Serialize};

use crate::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MergePassTelemetry {
    pub(crate) at: DateTime<Utc>,
    pub(crate) duration_ms: u64,
    pub(crate) repos_evaluated: usize,
    pub(crate) slowest_repo: Option<String>,
    pub(crate) slowest_repo_ms: u64,
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Records one pass. `slowest` is the repository whose own evaluation took the
/// longest, and how long — `None` when the pass evaluated no repository at all.
pub(crate) fn record(
    path: &Path,
    at: DateTime<Utc>,
    duration: Duration,
    repos_evaluated: usize,
    slowest: Option<(String, Duration)>,
) -> Result<()> {
    let (slowest_repo, slowest_repo_ms) = match slowest {
        Some((repo, elapsed)) => (Some(repo), millis(elapsed)),
        None => (None, 0),
    };
    let telemetry = MergePassTelemetry {
        at,
        duration_ms: millis(duration),
        repos_evaluated,
        slowest_repo,
        slowest_repo_ms,
    };
    let temp_path = path.with_extension(format!("{}.tmp", nanoid!()));
    let written = fs::write(&temp_path, serde_json::to_vec(&telemetry)?)
        .and_then(|()| fs::rename(&temp_path, path));
    if let Err(error) = written {
        let _ = fs::remove_file(temp_path);
        return Err(error.into());
    }
    Ok(())
}

/// The last recorded pass, or `None` when the daemon has never completed an
/// auto-merge pass (auto-merge disabled, or no pass has run yet).
pub(crate) fn load(path: &Path) -> Option<MergePassTelemetry> {
    let raw = fs::read(path).ok()?;
    serde_json::from_slice(&raw).ok()
}

#[cfg(test)]
#[path = "merge_pass_telemetry_tests.rs"]
mod tests;
