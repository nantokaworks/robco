//! The ledger entry's own lifecycle phase, split out of `ledger.rs` to keep
//! that file under this project's source file size limit: the phase enum
//! itself, its serialized label, and the small predicates that read a phase
//! alongside the one other field a phase transition leaves behind — whether
//! the entry is waiting on a prerequisite.

use serde::{Deserialize, Serialize};

use super::LedgerEntry;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LedgerPhase {
    Dispatched,
    Claimed,
    Working,
    PrOpened,
    Merged,
    Failed,
    Escalated,
}

impl LedgerPhase {
    /// The phase's name as every reader spells it: the CLI status line, the
    /// OVERSEER frame, the review digest, and the serialized ledger. They must
    /// agree, or the same board reads differently depending on where you look.
    pub fn label(self) -> &'static str {
        match self {
            Self::Dispatched => "dispatched",
            Self::Claimed => "claimed",
            Self::Working => "working",
            Self::PrOpened => "pr_opened",
            Self::Merged => "merged",
            Self::Failed => "failed",
            Self::Escalated => "escalated",
        }
    }
}

/// A phase no worker can leave: the entry no longer holds anything.
pub fn terminal(phase: LedgerPhase) -> bool {
    matches!(
        phase,
        LedgerPhase::Merged | LedgerPhase::Failed | LedgerPhase::Escalated
    )
}

/// Whether `entry` occupies a dispatch slot or a branch/worktree another
/// worker could collide with.
///
/// A terminal entry never does. Nor does one waiting on a prerequisite: the
/// worker that reported it already stepped aside (or, on the merge-gate
/// path, never held a worker slot in the first place), and dropr's own ready
/// feed excludes the task until the prerequisite closes — see dropr:375. So
/// neither the dispatch gate nor `active_workers`'s capacity count has
/// anything left to hold this entry against.
pub fn holds_capacity(entry: &LedgerEntry) -> bool {
    !terminal(entry.phase) && entry.prerequisite_wait.is_none()
}

/// Whether `entry` is currently idle, waiting on a dropr `blocks` dependency
/// edge to resolve. Read by `robco overseer status --debug` and by
/// `monitor::apply::apply_prerequisite_wait`'s escalation bound.
pub fn waiting_on_prerequisite(entry: &LedgerEntry) -> bool {
    entry.prerequisite_wait.is_some() && !terminal(entry.phase)
}
