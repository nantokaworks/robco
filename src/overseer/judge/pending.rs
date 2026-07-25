//! The judgment queue's durable half.
//!
//! `pending` and `active` lived only in the daemon's memory, so a restart lost
//! every question that was waiting and the one that was running — along with the
//! model session already paid for. The counters beside them were already durable
//! (`DailyCounter` in `queue.json`, `RevisionCache` in `revisions.json`); the
//! queue was the outlier.
//!
//! Deliberately a separate file from `queue_state.json`. That one is the
//! operator-facing snapshot `robco overseer status` reads from another process,
//! holding labels rather than requests; making it authoritative would have meant
//! one file serving a display contract and a recovery contract at once, where
//! any change to either has to keep faith with the other.
//!
//! The active request is saved at the front of the list, and comes back
//! *pending*: its session process did not survive the restart, so it must be
//! re-run rather than waited on. What keeps that from re-buying a verdict is
//! [`super::queue::JudgmentQueue::advance`], which consumes a `result.json` the
//! case directory already holds instead of spawning.

use std::{collections::VecDeque, fs, io::ErrorKind, path::Path};

use nanoid::nanoid;
use serde::{Deserialize, Serialize};

use super::Request;
use crate::Result;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct PendingQueue {
    /// Questions still to be asked, in service order.
    pub(super) requests: Vec<Request>,
}

impl PendingQueue {
    /// Reads the durable queue, degrading to an empty one rather than failing
    /// the daemon.
    ///
    /// The same tolerance `QueueSnapshot::load` and `RevisionCache::load`
    /// already have: a state file that cannot be read costs at most a re-run of
    /// the questions it described, while refusing to start costs the board.
    pub(super) fn load(path: &Path) -> Result<Self> {
        match fs::read(path) {
            Ok(raw) => Ok(serde_json::from_slice(&raw).unwrap_or_default()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    /// Captures the queue's live state: the running request first, then the
    /// waiting ones.
    pub(super) fn capture(active: Option<&Request>, pending: &VecDeque<Request>) -> Self {
        Self {
            requests: active.into_iter().chain(pending.iter()).cloned().collect(),
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
}
