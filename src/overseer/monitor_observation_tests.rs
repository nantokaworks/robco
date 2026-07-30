//! How the reconcile pass treats an observation it could not take.
//!
//! Kept beside `monitor_tests.rs` rather than in it: these cover the probe
//! failure path, which has its own subject — attribution — while the main suite
//! covers phase transitions.

use super::{Action, Observations, reconcile, tests::ledger, types::ObservationError};
use chrono::{TimeZone, Utc};

/// A probe that genuinely failed is still recorded — and now says which entry it
/// failed for. Recording every probe failure with no task and no repository is
/// what left an operator able to see that something broke but not what.
#[test]
fn a_failed_probe_names_the_entry_it_failed_for() {
    let observations = Observations {
        errors: vec![
            ObservationError::new("gh PR probe exited exit status: 1").about("task-131", "/repo"),
            ObservationError::new("inbox read failed: no such file"),
        ],
        ..Observations::default()
    };
    let now = Utc.with_ymd_and_hms(2026, 7, 16, 0, 2, 0).unwrap();
    let (_, actions) = reconcile(&ledger(), &observations, now, 30, 72);
    let logged: Vec<_> = actions
        .iter()
        .filter_map(|action| match action {
            Action::LogDecision { task_id, message } => Some((task_id.as_deref(), message.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        logged[0],
        (
            Some("task-131"),
            "observation skipped in /repo: gh PR probe exited exit status: 1".to_string()
        )
    );
    // A probe that belongs to no single entry keeps the unattributed form.
    assert_eq!(
        logged[1],
        (
            None,
            "observation skipped: inbox read failed: no such file".to_string()
        )
    );
}
