use super::*;
use crate::overseer::ledger::{LedgerEntry, LedgerPhase};

fn now() -> DateTime<Utc> {
    "2026-07-29T00:00:00Z".parse().unwrap()
}

fn entry(repo: &str, branch: &str) -> LedgerEntry {
    LedgerEntry {
        task_id: "task".into(),
        dropr_task_id: None,
        display_id: "#1".into(),
        repo: repo.into(),
        agent_id: "agent".into(),
        branch: branch.into(),
        phase: LedgerPhase::Merged,
        dispatched_at: now(),
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
fn a_branch_overseer_dispatched_is_not_someone_elses_pull_request() {
    let ledger = Ledger {
        entries: vec![entry("/repos/nex", "task-350")],
        ..Default::default()
    };
    assert!(dispatched_by_overseer(&ledger, "/repos/nex", "task-350"));
}

/// Settled, not just live: a merged or failed entry already surfaces in the
/// HISTORY section, and must not also appear as if it arrived on its own.
#[test]
fn a_settled_branch_is_still_overseers_own() {
    let ledger = Ledger {
        entries: vec![entry("/repos/nex", "task-350")],
        ..Default::default()
    };
    assert_eq!(
        ledger.entries[0].phase,
        LedgerPhase::Merged,
        "fixture must stay settled for this case to mean anything"
    );
    assert!(dispatched_by_overseer(&ledger, "/repos/nex", "task-350"));
}

#[test]
fn a_dependabot_branch_is_someone_elses_pull_request() {
    let ledger = Ledger {
        entries: vec![entry("/repos/nex", "task-350")],
        ..Default::default()
    };
    assert!(!dispatched_by_overseer(
        &ledger,
        "/repos/nex",
        "dependabot/npm_and_yarn/foo"
    ));
}

#[test]
fn a_matching_branch_in_a_different_repo_does_not_count() {
    let ledger = Ledger {
        entries: vec![entry("/repos/other", "task-350")],
        ..Default::default()
    };
    assert!(!dispatched_by_overseer(&ledger, "/repos/nex", "task-350"));
}

#[test]
fn a_repository_never_polled_is_due() {
    assert!(is_due(None, now()));
}

#[test]
fn a_repository_polled_within_the_refresh_interval_is_not_due() {
    let cached = RepoOtherPrs {
        polled_at: now() - chrono::Duration::minutes(1),
        prs: Vec::new(),
    };
    assert!(!is_due(Some(&cached), now()));
}

#[test]
fn a_repository_polled_before_the_refresh_interval_is_due_again() {
    let cached = RepoOtherPrs {
        polled_at: now() - REFRESH_INTERVAL,
        prs: Vec::new(),
    };
    assert!(is_due(Some(&cached), now()));
}

#[test]
fn closes_task_reads_the_directive_worker_prompts_are_told_to_write() {
    assert_eq!(
        closes_task("Fixes the bug.\n\nClose Dropr: #803\n"),
        Some("#803".into())
    );
}

#[test]
fn closes_task_is_case_insensitive() {
    assert_eq!(closes_task("close dropr: #803"), Some("#803".into()));
    assert_eq!(closes_task("CLOSE DROPR: #803"), Some("#803".into()));
}

#[test]
fn closes_task_tolerates_a_missing_hash() {
    assert_eq!(closes_task("Close Dropr: 803"), Some("#803".into()));
}

#[test]
fn closes_task_is_none_without_the_directive() {
    assert_eq!(closes_task("Bumps foo from 1.0 to 1.1."), None);
}

#[test]
fn closes_task_is_none_when_no_number_follows() {
    assert_eq!(closes_task("Close Dropr: nothing here"), None);
}

fn registry_with(path: &str) -> Registry {
    Registry {
        version: 1,
        repos: vec![crate::discover::repo_node(path.into(), false)],
    }
}

fn other_prs_entry() -> RepoOtherPrs {
    RepoOtherPrs {
        polled_at: now(),
        prs: Vec::new(),
    }
}

#[test]
fn prune_unregistered_drops_a_path_the_registry_no_longer_lists() {
    let mut state = OtherPrs::default();
    state
        .repos
        .insert("/repos/renamed-away".into(), other_prs_entry());
    state.repos.insert("/repos/robco".into(), other_prs_entry());

    prune_unregistered(&mut state, &registry_with("/repos/robco"));

    assert_eq!(state.repos.keys().collect::<Vec<_>>(), vec!["/repos/robco"]);
}

#[test]
fn prune_unregistered_keeps_every_registered_path() {
    let mut state = OtherPrs::default();
    state.repos.insert("/repos/robco".into(), other_prs_entry());

    prune_unregistered(&mut state, &registry_with("/repos/robco"));

    assert_eq!(state.repos.len(), 1);
}

#[test]
fn closes_task_does_not_panic_on_multibyte_text_before_the_directive() {
    // A body with non-ASCII text ahead of the marker must not shift the byte
    // offset `closes_task` slices `body` at — `to_ascii_lowercase` guards
    // this, but a regression back to `to_lowercase()` could reintroduce a
    // char-boundary panic here.
    assert_eq!(
        closes_task("日本語のコメント İstanbul\n\nClose Dropr: #803"),
        Some("#803".into())
    );
}
