use super::*;

fn now() -> DateTime<Utc> {
    "2026-07-28T00:00:00Z".parse().unwrap()
}

fn entry(phase: LedgerPhase, pr_url: Option<&str>) -> LedgerEntry {
    entry_settled(phase, pr_url, None)
}

fn entry_settled(
    phase: LedgerPhase,
    pr_url: Option<&str>,
    settled_at: Option<DateTime<Utc>>,
) -> LedgerEntry {
    LedgerEntry {
        task_id: "task".into(),
        display_id: "#1".into(),
        repo: "/repo".into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
        phase,
        dispatched_at: chrono::Utc::now(),
        settled_at,
        retries: 0,
        pr_url: pr_url.map(str::to_owned),
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        manual_merge_skip: None,
        merge_judge_fail_safes: 0,
        merge_hold_cap_escalated: false,
        merge_hold_rechecks: 0,
        merge_hold_recheck_reason: None,
        merge_hold_recheck_head: None,
    }
}

fn pr(state: &str) -> PrObservation {
    PrObservation {
        state: state.into(),
        url: Some(format!("https://pr/{state}")),
        ..PrObservation::default()
    }
}

/// The case that used to write an error into `decisions.jsonl` on every
/// pass: a worker is still implementing and has not opened its pull request.
#[test]
fn a_branch_with_no_pull_request_is_a_state_not_an_error() {
    assert_eq!(best_pr(Vec::new()), None);
}

/// An escalation or a failure an operator answered by merging the pull
/// request themselves used to be invisible for `failed`, and only worked
/// for `escalated` while a pull request was already recorded: the entry
/// was terminal, so it was never read again.
#[test]
fn escalated_and_failed_entries_are_still_read() {
    let url = Some("https://pr/1");
    for terminal in [LedgerPhase::Escalated, LedgerPhase::Failed] {
        assert!(worth_probing(&entry(terminal, url), now()));
        // No recorded pull request is no longer a reason to skip: the
        // branch itself is checked, in case one was opened after all.
        assert!(worth_probing(&entry(terminal, None), now()));
    }
    // Cleanup has already run for a merged entry; re-reading it would
    // only cost a call for an answer nothing acts on.
    assert!(!worth_probing(&entry(LedgerPhase::Merged, url), now()));
    for live in [
        LedgerPhase::Dispatched,
        LedgerPhase::Claimed,
        LedgerPhase::Working,
        LedgerPhase::PrOpened,
    ] {
        assert!(
            worth_probing(&entry(live, None), now()),
            "{live:?} is still live"
        );
    }
}

/// The bound: a settled entry stops costing a `gh` call once it has been
/// stale for longer than `RECONCILE_MAX_SETTLED_AGE`, so a branch that
/// never merges does not cost a round trip on every poll forever.
#[test]
fn a_settled_entry_stops_being_probed_past_the_reconcile_age_cap() {
    let recent = now() - (RECONCILE_MAX_SETTLED_AGE - Duration::days(1));
    let stale = now() - (RECONCILE_MAX_SETTLED_AGE + Duration::days(1));
    for terminal in [LedgerPhase::Escalated, LedgerPhase::Failed] {
        assert!(worth_probing(
            &entry_settled(terminal, None, Some(recent)),
            now()
        ));
        assert!(!worth_probing(
            &entry_settled(terminal, None, Some(stale)),
            now()
        ));
        // No recorded settle time — an entry that reached the phase
        // before the field existed — has no age to compare and stays
        // eligible, so the historical backlog this bound exists to clear
        // is not the backlog it silently stops clearing.
        assert!(worth_probing(&entry_settled(terminal, None, None), now()));
    }
}

/// The #283 shape from dropr task #335: a terminal entry's recorded
/// `pr_url` can point at a pull request that closed unmerged while the
/// work actually landed as a different pull request on the same branch.
/// Identity for a terminal entry has to be the branch, never the URL.
#[test]
fn a_terminal_entry_is_identified_by_branch_not_its_recorded_pr_url() {
    for terminal in [LedgerPhase::Escalated, LedgerPhase::Failed] {
        assert!(probe_by_branch(&entry(terminal, Some("https://pr/1"))));
    }
    for live in [
        LedgerPhase::Dispatched,
        LedgerPhase::Claimed,
        LedgerPhase::Working,
        LedgerPhase::PrOpened,
    ] {
        assert!(!probe_by_branch(&entry(live, Some("https://pr/1"))));
    }
}

#[test]
fn an_open_pull_request_outranks_earlier_attempts() {
    let best = best_pr(vec![pr("CLOSED"), pr("OPEN"), pr("MERGED")]).unwrap();
    assert_eq!(best.state, "OPEN");
    // A merge that landed still outweighs an attempt that was abandoned, so
    // a reopened-then-closed branch does not read as unmerged.
    let best = best_pr(vec![pr("CLOSED"), pr("MERGED")]).unwrap();
    assert_eq!(best.state, "MERGED");
}
