//! dropr:462 — whether a row shows the board reviewer's stored sentence,
//! split out of `inbox_tests.rs` to keep that file under the size limit.

use chrono::TimeZone;

use super::*;
use crate::{
    overseer::{
        dismissals::Dismissals,
        ledger::{Ledger, LedgerEntry, LedgerPhase},
        row_summaries::{RowSummaries, RowSummary},
    },
    registry::Registry,
};

fn at(second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, second).unwrap()
}

fn escalated_ledger() -> Ledger {
    Ledger {
        entries: vec![LedgerEntry {
            task_id: "task-1".into(),
            display_id: "#159".into(),
            repo: "robco".into(),
            agent_id: "agent-1".into(),
            branch: "task-159".into(),
            phase: LedgerPhase::Escalated,
            dispatched_at: at(1),
            settled_at: None,
            retries: 0,
            pr_url: None,
            branch_updates: 0,
            merge_recovery: Default::default(),
            merge_hold: Default::default(),
            manual_merge_skip: None,
            merge_hold_cap_escalated: false,
            merge_hold_rechecks: 0,
            merge_hold_recheck_reason: None,
            merge_hold_recheck_head: None,
            prerequisite_wait: None,
            merge_hold_stuck_notified: false,
            escalation_notified_reason: None,
            escalation_notified_head: None,
            worker_escalated: false,
            operator_override: None,
            merge_approval: None,
            pr_facts: None,
        }],
        ..Ledger::default()
    }
}

fn aggregated_item(ledger: &Ledger) -> InboxItem {
    aggregate(
        ledger,
        &[],
        &[],
        &Dismissals::default(),
        &Registry::default(),
        &RowSummaries::default(),
    )
    .items
    .remove(0)
}

/// A row whose stored summary still matches its current case shows the
/// board reviewer's sentence.
#[test]
fn a_row_shows_a_stored_summary_that_still_matches_its_case() {
    let ledger = escalated_ledger();
    let item = aggregated_item(&ledger);
    let mut summaries = RowSummaries::default();
    summaries.upsert(
        item.target_id.clone(),
        RowSummary {
            sentence: "adds a merge approval queue".into(),
            signature: item.case_signature(),
            generated_at: Utc::now(),
        },
    );

    let inbox = aggregate(
        &ledger,
        &[],
        &[],
        &Dismissals::default(),
        &Registry::default(),
        &summaries,
    );

    assert_eq!(
        inbox.items[0].sentence.as_deref(),
        Some("adds a merge approval queue")
    );
}

/// A summary written for an older revision of the same target must not be
/// shown once the case has changed underneath it.
#[test]
fn a_stale_summary_is_not_shown() {
    let ledger = escalated_ledger();
    let item = aggregated_item(&ledger);
    let mut summaries = RowSummaries::default();
    summaries.upsert(
        item.target_id.clone(),
        RowSummary {
            sentence: "describes an old case".into(),
            signature: "a signature that does not match the current case".into(),
            generated_at: Utc::now(),
        },
    );

    let inbox = aggregate(
        &ledger,
        &[],
        &[],
        &Dismissals::default(),
        &Registry::default(),
        &summaries,
    );

    assert_eq!(inbox.items[0].sentence, None);
}
