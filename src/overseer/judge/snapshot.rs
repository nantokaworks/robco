//! Operator-visible snapshot of the judgment queue.
//!
//! The queue itself lives in the daemon's memory, so `robco overseer status` —
//! a separate process — can only learn what it is doing from a file the daemon
//! keeps current. Without it, a pull request queued behind another judgment
//! looks exactly like a daemon that stopped judging, which is the same
//! ambiguity the `judge_pending` decision removes from `decisions.jsonl`.

use crate::Result;
use nanoid::nanoid;
use serde::{Deserialize, Serialize};
use std::{fs, io::ErrorKind, path::Path};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueSnapshot {
    /// Labels of the judgments currently running, one per occupied slot —
    /// at most one dispatch round plus at most one merge judgment per
    /// repository. See `crate::overseer::judge::Request::slot`.
    #[serde(default)]
    pub active: Vec<String>,
    /// Labels of the judgments waiting behind them, in service order.
    #[serde(default)]
    pub pending: Vec<String>,
}

impl QueueSnapshot {
    pub(super) fn load(path: &Path) -> Result<Self> {
        match fs::read(path) {
            Ok(raw) => Ok(serde_json::from_slice(&raw).unwrap_or_default()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp = path.with_extension(format!("json.{}.tmp", nanoid!()));
        let raw = serde_json::to_vec_pretty(self)?;
        if let Err(error) = fs::write(&temp, raw).and_then(|()| fs::rename(&temp, path)) {
            let _ = fs::remove_file(temp);
            return Err(error.into());
        }
        Ok(())
    }

    /// One-line rendering for `robco overseer status`.
    pub fn summary(&self) -> String {
        let active = if self.active.is_empty() {
            "none".to_owned()
        } else {
            self.active.join(", ")
        };
        let waiting = if self.pending.is_empty() {
            String::new()
        } else {
            format!(" ({})", self.pending.join(", "))
        };
        format!(
            "judge queue: active {active}  waiting {}{waiting}",
            self.pending.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_queued_judgment_is_distinguishable_from_an_idle_queue() {
        let idle = QueueSnapshot::default();
        assert_eq!(idle.summary(), "judge queue: active none  waiting 0");
        let busy = QueueSnapshot {
            active: vec!["merge:task-1".into()],
            pending: vec!["merge:task-2".into()],
        };
        assert_eq!(
            busy.summary(),
            "judge queue: active merge:task-1  waiting 1 (merge:task-2)"
        );
    }

    /// Two merge judgments in different repositories are both `active` at
    /// once — the whole point of the per-repository queue.
    #[test]
    fn concurrent_judgments_across_repositories_are_all_reported_active() {
        let busy = QueueSnapshot {
            active: vec!["merge:task-1".into(), "merge:task-2".into()],
            pending: Vec::new(),
        };
        assert_eq!(
            busy.summary(),
            "judge queue: active merge:task-1, merge:task-2  waiting 0"
        );
    }

    #[test]
    fn snapshot_survives_the_process_that_wrote_it() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("queue_state.json");
        assert_eq!(
            QueueSnapshot::load(&path).unwrap(),
            QueueSnapshot::default()
        );
        let snapshot = QueueSnapshot {
            active: vec!["dispatch:2".into(), "merge:task-1".into()],
            pending: vec!["merge:task-2".into()],
        };
        snapshot.save(&path).unwrap();
        assert_eq!(QueueSnapshot::load(&path).unwrap(), snapshot);
    }

    #[test]
    fn unreadable_snapshot_reads_as_idle_rather_than_failing_status() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("queue_state.json");
        fs::write(&path, b"{not json").unwrap();
        assert_eq!(
            QueueSnapshot::load(&path).unwrap(),
            QueueSnapshot::default()
        );
    }
}
