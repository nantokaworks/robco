//! Small `run_daemon` helpers split out of `daemon.rs` to keep that file
//! focused on the poll loop itself. See `daemon::terminal`'s re-export for why
//! this stays reachable at the parent's path.

use crate::Result;
use crate::overseer::{
    config_write,
    ledger::{Ledger, LedgerPhase},
    logging,
    monitor::{Action, FailureOrigin},
};

/// Write back what the drained requests changed. `runtime_request::apply` only
/// ever flips `overseer.dispatch_enabled`, so the write narrows to that field
/// rather than serialising this pass's snapshot over an operator's edits.
pub(crate) fn persist_drained_config(dispatch_enabled: bool) -> Result<()> {
    if config_write::persist_dispatch_enabled(dispatch_enabled)? {
        logging::log_message(
            None,
            &format!("config rewritten: overseer.dispatch_enabled={dispatch_enabled}"),
        )?;
    }
    Ok(())
}

pub(crate) fn account_failures(previous: &Ledger, next: &mut Ledger, actions: &[Action]) {
    // Only worker-origin failures count; merges reset the streak, while re-arm
    // otherwise remains an operator action.
    let failures = actions
        .iter()
        .filter(|action| {
            matches!(
                action,
                Action::MarkFailed {
                    origin: FailureOrigin::Worker,
                    ..
                }
            )
        })
        .count() as u32;
    next.counters.consecutive_failures =
        next.counters.consecutive_failures.saturating_add(failures);
    let newly_merged = next.entries.iter().any(|entry| {
        entry.phase == LedgerPhase::Merged
            && previous
                .entries
                .iter()
                .find(|old| old.task_id == entry.task_id)
                .is_some_and(|old| old.phase != LedgerPhase::Merged)
    });
    if newly_merged {
        next.counters.consecutive_failures = 0;
    }
}

pub(crate) fn terminal(phase: LedgerPhase) -> bool {
    matches!(
        phase,
        LedgerPhase::Merged | LedgerPhase::Failed | LedgerPhase::Escalated
    )
}
