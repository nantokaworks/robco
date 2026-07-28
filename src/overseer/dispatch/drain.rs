//! Queue-drained detection: the Overseer has nothing left to do when the
//! ready-candidate gather succeeds empty *and* no ledger entry is still in
//! flight. Edge-triggered — logged once per transition into that state, not
//! once per poll — and the edge is persisted so a daemon restart neither
//! re-announces a drain that already fired nor announces one that never
//! happened.
//!
//! Re-arming is purely state-transition based: the next `queue_drained`
//! only fires after a pass observes the board *not* drained (new work
//! appeared) and then drained again. No separate debounce timer sits on
//! top of that — a board oscillating between "one ready task" and "zero"
//! would need actual dispatch-and-settle cycles to flip the persisted
//! state, and a settle cycle already spans many poll intervals, so a timer
//! would only add complexity without a failure mode it actually prevents.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::overseer::{
    ledger::{Ledger, terminal},
    logging::{DecisionEntry, DecisionKind},
    statefile::{atomic_replace, with_lock},
};

use super::Candidate;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
struct DrainedState {
    /// Whether the board was already drained as of the last recorded pass.
    drained: bool,
}

impl Default for DrainedState {
    // Unknown history reads as "already calm", not as "was busy": a
    // first-ever pass — whether the board starts empty or full — must never
    // fire a spurious "just drained" edge.
    fn default() -> Self {
        Self { drained: true }
    }
}

impl DrainedState {
    fn load(path: &Path) -> Result<Self> {
        match std::fs::read(path) {
            Ok(raw) => Ok(serde_json::from_slice(&raw).unwrap_or_else(|error| {
                eprintln!("warning: ignoring unreadable queue-drained state: {error}");
                Self::default()
            })),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    fn save(&self, path: &Path) -> Result<()> {
        atomic_replace(path, &serde_json::to_vec_pretty(self)?)
    }
}

/// Pure predicate: is the board drained right now? Only meaningful when the
/// candidate gather that produced `candidates` actually succeeded — a
/// caller whose gather failed (e.g. `dropr_overlay_unavailable`) must not
/// call this, or a transient outage reads as "all done".
fn is_drained(candidates: &[Candidate], ledger: &Ledger) -> bool {
    candidates.is_empty() && ledger.entries.iter().all(|entry| terminal(entry.phase))
}

/// Checks whether the board just crossed into "drained" and, if so, logs the
/// `queue_drained` daemon event exactly once. Call only after a candidate
/// gather that succeeded; a failed gather must skip this entirely; calling
/// it with a failed gather's (necessarily empty) candidate list would either
/// falsely announce a drain during an outage or falsely clear a real one.
pub(super) fn check(candidates: &[Candidate], ledger: &Ledger) -> Result<()> {
    check_at(
        &super::super::queue_drained_state_path()?,
        candidates,
        ledger,
        crate::overseer::logging::append,
    )
}

fn check_at<F>(path: &Path, candidates: &[Candidate], ledger: &Ledger, mut append: F) -> Result<()>
where
    F: FnMut(&DecisionEntry) -> Result<()>,
{
    with_lock(path, || {
        let mut state = DrainedState::load(path)?;
        let now_drained = is_drained(candidates, ledger);
        if now_drained && !state.drained {
            let mut entry = DecisionEntry::new(DecisionKind::Skip, "queue_drained");
            entry.source = Some("daemon_event".into());
            append(&entry)?;
        }
        if state.drained != now_drained {
            state.drained = now_drained;
            state.save(path)?;
        }
        Ok(())
    })
}

#[cfg(test)]
#[path = "drain_tests.rs"]
mod tests;
