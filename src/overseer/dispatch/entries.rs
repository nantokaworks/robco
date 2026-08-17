//! How the dispatch gate reads the ledger: which entries belong to a candidate,
//! whether one of them is still live, and which worker owns it.

use std::collections::HashMap;

use crate::model::ManagementMode;
use crate::overseer::ledger::{Ledger, LedgerEntry};

pub(super) use crate::overseer::ledger::holds_capacity;

/// Ledger entries recording an attempt at this task. Both identifiers are
/// compared because a ready task without a nanoid is tracked under its display
/// id (see `gather_candidates`), so neither field alone matches every entry.
pub(super) fn task_entries<'a>(
    ledger: &'a Ledger,
    task_id: &'a str,
    display_id: &'a str,
) -> impl Iterator<Item = &'a LedgerEntry> + 'a {
    ledger
        .entries
        .iter()
        .filter(move |entry| entry.task_id == task_id || entry.display_id == display_id)
}

/// True while a worker still carries this task through a non-terminal phase
/// and is not merely waiting on a prerequisite — see
/// `ledger::holds_capacity`.
pub(super) fn has_active_worker(ledger: &Ledger, task_id: &str, display_id: &str) -> bool {
    task_entries(ledger, task_id, display_id).any(holds_capacity)
}

pub(super) fn worker_mode(
    entry: &LedgerEntry,
    worker_modes: &HashMap<String, ManagementMode>,
) -> ManagementMode {
    worker_modes
        .get(&entry.agent_id)
        .copied()
        .unwrap_or(ManagementMode::Auto)
}
