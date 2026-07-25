//! How one pull request's turn through the auto-merge gate is named and recorded.
//!
//! The gate has many exits — a failed read, an unprotected base, a red check, a
//! non-mergeable state, a judge's veto, a refused merge — and two readers care
//! which one it took: `decisions.jsonl`, and the merge-recovery step that decides
//! whether the failure is the owning worker's to fix. Naming every exit in one
//! place is what lets both read the same reason.

use crate::{
    Result,
    overseer::{
        config::ProtectionMode,
        ledger::LedgerEntry,
        logging::{self, DecisionEntry, DecisionKind},
    },
};

/// How one pull request's turn through the gate ended.
pub(super) enum Outcome {
    /// GitHub accepted the merge.
    Merged,
    /// The pass stopped under `halt`, on the revision named by `head`.
    Halted { halt: Halt, head: String },
    /// The pass stopped without a decision of its own, because the merge judgment
    /// is still queued. There is no failure yet, so nothing is handed back.
    Pending,
}

/// A recorded non-merge outcome. The reason is both what `decisions.jsonl` shows
/// and, when the failure is one a worker can fix, the instruction it receives.
pub(super) struct Halt {
    pub(super) kind: DecisionKind,
    pub(super) reason: String,
    /// Whether the active strictness mode is recorded alongside the decision.
    gated: bool,
}

impl Halt {
    pub(super) fn hold(reason: impl Into<String>) -> Self {
        Self {
            kind: DecisionKind::Hold,
            reason: reason.into(),
            gated: false,
        }
    }

    pub(super) fn escalate(reason: impl Into<String>) -> Self {
        Self {
            kind: DecisionKind::Escalate,
            reason: reason.into(),
            gated: false,
        }
    }

    pub(super) fn gated(reason: impl Into<String>) -> Self {
        Self {
            gated: true,
            ..Self::hold(reason)
        }
    }

    pub(super) fn on(self, head: &str) -> Outcome {
        Outcome::Halted {
            halt: self,
            head: head.to_owned(),
        }
    }
}

/// Reason recorded for a merge candidate left alone because its worker is
/// manual-managed.
const MANUAL_MANAGED: &str = "manual";

/// What the management gate has to say about `entry`, and the marker that keeps
/// it from saying it again next pass.
///
/// A manual worker belongs to a human, so declining the merge is correct and does
/// not change here — only its silence does. The decision is recorded once per pull
/// request rather than once per pass: manual management is a standing state, and at
/// the poll cadence a per-pass entry would bury the log exactly the way the silent
/// skip hid in it. The pull request URL is the deduplication key, where a handback
/// uses the head sha: this gate stops before reading the pull request, so no head is
/// known, and buying one costs a GitHub read every pass for a pull request it will
/// never merge. An entry with no pull request has nothing to decline yet, so it stays
/// quiet until one is open.
pub(super) fn manual_skip(entry: &mut LedgerEntry, auto: bool) -> Option<DecisionEntry> {
    if auto {
        // Overseer's to merge again: drop the marker so a later switch back to
        // manual records its own skip instead of being swallowed as a repeat.
        entry.manual_merge_skip = None;
        return None;
    }
    let url = entry.pr_url.clone()?;
    if entry.manual_merge_skip.as_deref() == Some(url.as_str()) {
        return None;
    }
    // The same word the dispatch gate uses for the same condition, under this
    // gate's own source, so the two are greppable together and still separable.
    let mut skip = decision(entry, DecisionKind::Skip, MANUAL_MANAGED);
    skip.pr_url = Some(url.clone());
    entry.manual_merge_skip = Some(url);
    Some(skip)
}

pub(super) fn log(entry: &LedgerEntry, kind: DecisionKind, reason: &str) -> Result<()> {
    logging::append(&decision(entry, kind, reason))
}

pub(super) fn log_halt(entry: &LedgerEntry, halt: &Halt, mode: ProtectionMode) -> Result<()> {
    if halt.gated {
        log_gated(entry, halt.kind, &halt.reason, mode)
    } else {
        log(entry, halt.kind, &halt.reason)
    }
}

pub(super) fn log_gated(
    entry: &LedgerEntry,
    kind: DecisionKind,
    reason: &str,
    mode: ProtectionMode,
) -> Result<()> {
    logging::append(&gated_decision(entry, kind, reason, mode))
}

/// Records the active strictness mode alongside the decision, so a merge that only
/// happened because the gate was loosened stays distinguishable in `decisions.jsonl`.
pub(super) fn gated_decision(
    entry: &LedgerEntry,
    kind: DecisionKind,
    reason: &str,
    mode: ProtectionMode,
) -> DecisionEntry {
    let mut decision = decision(entry, kind, reason);
    decision.protection_mode = Some(mode.label().to_owned());
    decision
}

pub(super) fn decision(entry: &LedgerEntry, kind: DecisionKind, reason: &str) -> DecisionEntry {
    let mut decision = DecisionEntry::new(kind, reason);
    decision.task = Some(entry.task_id.clone());
    decision.repo = Some(entry.repo.clone());
    decision.source = Some("auto_merge".into());
    decision
}

#[cfg(test)]
#[path = "merge_decision_tests.rs"]
mod tests;
