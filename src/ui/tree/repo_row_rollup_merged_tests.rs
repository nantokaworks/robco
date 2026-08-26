//! `collapsed_rollup`'s ledger-aware substitution (dropr:563). Split out of
//! `repo_row.rs`'s own `mod tests` to keep that file at its size limit.

use chrono::Utc;

use super::*;
use crate::{
    overseer::ledger::{Ledger, LedgerEntry, LedgerPhase},
    ui::test_support,
};

fn merged_entry(agent_id: &str) -> LedgerEntry {
    LedgerEntry {
        task_id: "task-1".into(),
        dropr_task_id: None,
        display_id: "#1".into(),
        repo: "nantokaworks/robco".into(),
        agent_id: agent_id.into(),
        branch: "branch".into(),
        phase: LedgerPhase::Merged,
        dispatched_at: Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: None,
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
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
        worker_finished_at: None,
        approval_dropped: None,
    }
}

#[test]
fn a_merged_dead_agent_counts_under_done_not_dead() {
    let dir = std::path::PathBuf::from("/tmp/repo_row_rollup_merged_tests");
    let mut merged = test_support::agent("merged", dir.join("merged"));
    merged.status = Status::Dead;
    let mut crashed = test_support::agent("crashed", dir.join("crashed"));
    crashed.status = Status::Dead;
    let repo = test_support::repo(dir, vec![merged, crashed]);

    let mut ledger = Ledger::default();
    ledger.entries.push(merged_entry("merged"));

    let mut right = Vec::new();
    collapsed_rollup(&repo, &ledger, false, THEME.hint_style(), &mut right);

    let done_chunk = right
        .iter()
        .find(|span| span.content.contains(Status::Done.glyph()))
        .expect("the merged agent counts as done");
    assert!(done_chunk.content.starts_with('1'));

    let dead_chunk = right
        .iter()
        .find(|span| span.content.contains(Status::Dead.glyph()))
        .expect("the crashed agent still counts as dead");
    assert!(dead_chunk.content.starts_with('1'));
}
