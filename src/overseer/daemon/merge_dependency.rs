//! Whether a pull request's task carries an unresolved `blocks` dependency
//! edge — the merge-time counterpart to `dropr task ready` already excluding
//! such a task from dispatch's own candidate feed (dropr:375).
//!
//! A worker cannot un-open a pull request once it discovers, after the fact,
//! that its task was ordered behind another — so unlike the dispatch side,
//! which needs no code here at all (dropr already stops offering the task),
//! the auto-merge gate has to read the edge itself and hold the pull request
//! back.
//!
//! Split into a live [`probe`] and a pure [`apply`] the same way `value: &Value`
//! is fetched once by `merge_evaluate::evaluate` and then only read in memory by
//! `merge_gate::gate`: `apply` is what a test drives directly, and `probe` is
//! the one place that actually shells out.

use super::merge_decision::Halt;
use crate::overseer::{exec::COMMAND_TIMEOUT, ledger::LedgerEntry};

/// Reason prefix recorded while a pull request's task depends on another task
/// that has not closed yet. The suffix names the prerequisite by its display
/// id, so `decisions.jsonl` and the Inbox row say what this pull request is
/// actually waiting on.
///
/// `pub(crate)` so `merge_hold::bounded` (excludes it from that budget, the
/// same way it excludes `behind_`) and `remedy::table` (routes it to
/// `Move::Watch`, the way `merge_queue::WAITING_TURN` is routed) can both
/// name it.
pub(crate) const PREREQUISITE_UNMERGED_PREFIX: &str = "prerequisite_unmerged:";

/// Reason recorded when the dependency probe itself failed — a `dropr` binary
/// missing, a timeout, an unreadable response. Unlike
/// `PREREQUISITE_UNMERGED_PREFIX`, this is bounded normally by `merge_hold`:
/// a probe that never works is an operator's problem (a broken `dropr` on the
/// daemon's `PATH`), not a steady-state ordering wait, and must still
/// converge to an escalation instead of holding the pull request forever on
/// an answer nobody ever confirmed.
const PREREQUISITE_PROBE_FAILED: &str = "prerequisite_probe_failed";

/// What this pass learned about `entry`'s dependency edges.
pub(super) enum Probe {
    /// No unresolved `blocks` edge names this task as the dependent side.
    Clear,
    /// An unresolved `blocks` edge does; the prerequisite is named by its
    /// dropr display id.
    Blocking(String),
    /// The read itself did not answer.
    Failed,
}

/// Reads whether `entry`'s task still carries an unresolved `blocks`
/// dependency edge.
///
/// `dropr_task_id` is `LedgerEntry::dropr_task_id` — `None` for an entry with
/// no known dropr task, such as one `daemon::observations::adopt_registry_children_from`
/// filled straight from the agent record when Overseer never dispatched it
/// itself. Such an entry has no dependency edges by construction, so `None`
/// here answers `Clear` without asking dropr at all. That is not the same
/// thing as a probe that ran and could not answer: there being no such task
/// IS the answer, not a response nobody confirmed. See dropr:496, dropr:531.
///
/// Read fresh every pass rather than cached: the edge resolves itself the
/// moment its prerequisite closes, so a stale answer would hold a pull
/// request the prerequisite has already cleared.
pub(super) fn probe(dropr_task_id: Option<&str>) -> Probe {
    let Some(task_id) = dropr_task_id else {
        return Probe::Clear;
    };
    match crate::dropr::blocking_prerequisite_timeout(task_id, COMMAND_TIMEOUT) {
        Ok(Some(prerequisite)) => Probe::Blocking(prerequisite.display_id),
        Ok(None) => Probe::Clear,
        Err(_) => Probe::Failed,
    }
}

/// Applies a [`Probe`] already taken this pass: keeps `entry.prerequisite_wait`
/// in step with it, and names the halt when one is due.
///
/// Called first in `merge_gate::gate`, ahead of protection and checks, so a
/// pull request held on this never pays for probes whose answer cannot
/// change anything.
pub(super) fn apply(entry: &mut LedgerEntry, probe: Probe) -> Option<Halt> {
    match probe {
        Probe::Blocking(display_id) => {
            entry.prerequisite_wait.get_or_insert_with(chrono::Utc::now);
            Some(Halt::hold(format!(
                "{PREREQUISITE_UNMERGED_PREFIX}{display_id}"
            )))
        }
        Probe::Clear => {
            entry.prerequisite_wait = None;
            None
        }
        // A probe that did not answer must not be read as "nothing is
        // blocking" — that would let a pull request merge past an ordering
        // constraint the gate never actually confirmed was clear. Left
        // alone rather than cleared: `entry.prerequisite_wait` keeps
        // whatever it already recorded, so a transient failure between two
        // successful probes does not reset the escalation bound's clock.
        Probe::Failed => Some(Halt::hold(PREREQUISITE_PROBE_FAILED)),
    }
}

#[cfg(test)]
#[path = "merge_dependency_tests.rs"]
mod tests;
