use chrono::Utc;

use super::*;

fn decision(kind: DecisionKind, task: &str, reason: &str) -> DecisionEntry {
    let mut entry = DecisionEntry::new(kind, reason);
    entry.task = Some(task.into());
    entry
}

fn ledger_entry(
    agent_id: &str,
    task_id: &str,
    phase: LedgerPhase,
) -> crate::overseer::ledger::LedgerEntry {
    crate::overseer::ledger::LedgerEntry {
        task_id: task_id.into(),
        display_id: task_id.into(),
        repo: "nantokaworks/robco".into(),
        agent_id: agent_id.into(),
        branch: format!("task-{task_id}"),
        phase,
        dispatched_at: Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: None,
        branch_updates: 0,
        merge_judge_primes: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        manual_merge_skip: None,
        merge_judge_fail_safes: 0,
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
    }
}

fn escalated_ledger(agent_id: &str, task_id: &str) -> Ledger {
    let mut ledger = Ledger::default();
    ledger
        .entries
        .push(ledger_entry(agent_id, task_id, LedgerPhase::Escalated));
    ledger
}

fn pr_opened_ledger(agent_id: &str, task_id: &str, hold_reason: Option<&str>) -> Ledger {
    let mut ledger = Ledger::default();
    let mut entry = ledger_entry(agent_id, task_id, LedgerPhase::PrOpened);
    entry.merge_hold.reason = hold_reason.map(str::to_string);
    ledger.entries.push(entry);
    ledger
}

#[test]
fn a_freshly_escalated_worker_needs_a_decision() {
    let ledger = escalated_ledger("worker-1", "#351");
    let decisions = [decision(DecisionKind::Escalate, "#351", "worker blocked")];
    assert_eq!(
        blocked_reason(Locale::En, &ledger, &decisions, "worker-1"),
        Some("worker blocked".into())
    );
}

#[test]
fn a_worker_the_overseer_resolved_itself_is_not_flagged() {
    // The triage judge answered the worker's blocker on its own; it logs
    // `Hold` but never reverts the ledger phase out of `Escalated`
    // (`overseer::triage::completion`), so `Hold` is the only thing that
    // can tell this apart from a still-open escalation.
    let ledger = escalated_ledger("worker-1", "#351");
    let decisions = [
        decision(DecisionKind::Escalate, "#351", "worker blocked"),
        decision(DecisionKind::Hold, "#351", "answered via triage"),
    ];
    assert_eq!(
        blocked_reason(Locale::En, &ledger, &decisions, "worker-1"),
        None
    );
}

#[test]
fn a_skipped_case_is_not_flagged() {
    let ledger = escalated_ledger("worker-1", "#351");
    let decisions = [
        decision(DecisionKind::Escalate, "#351", "worker blocked"),
        decision(DecisionKind::Skip, "#351", "not a real blocker"),
    ];
    assert_eq!(
        blocked_reason(Locale::En, &ledger, &decisions, "worker-1"),
        None
    );
}

#[test]
fn a_non_escalated_entry_is_not_flagged() {
    let mut ledger = escalated_ledger("worker-1", "#351");
    ledger.entries[0].phase = LedgerPhase::Working;
    assert_eq!(blocked_reason(Locale::En, &ledger, &[], "worker-1"), None);
}

#[test]
fn an_unrelated_agent_is_not_flagged() {
    let ledger = escalated_ledger("worker-1", "#351");
    assert_eq!(blocked_reason(Locale::En, &ledger, &[], "worker-2"), None);
}

#[test]
fn a_missing_decision_log_still_flags_an_escalated_entry() {
    // The decision log is a bounded tail (`DECISION_SNAPSHOT_LIMIT`); an
    // old escalation can roll out of it while the ledger entry, which has
    // no such cap, is still `Escalated`. Default to still needing a
    // person rather than silently dropping the marker.
    let ledger = escalated_ledger("worker-1", "#351");
    assert_eq!(
        blocked_reason(Locale::En, &ledger, &[], "worker-1"),
        Some("worker blocked".into())
    );
}

#[test]
fn checks_waiting_reads_as_checks_running() {
    let ledger = pr_opened_ledger("worker-1", "#359", Some("checks_waiting"));
    assert_eq!(
        merge_lifecycle(&ledger, "worker-1"),
        Some(MergeLifecycle::ChecksRunning)
    );
}

#[test]
fn checks_not_green_reads_as_checks_failing() {
    let ledger = pr_opened_ledger("worker-1", "#359", Some("checks_not_green"));
    assert_eq!(
        merge_lifecycle(&ledger, "worker-1"),
        Some(MergeLifecycle::ChecksFailing)
    );
}

#[test]
fn a_merge_error_reads_as_checks_failing() {
    let ledger = pr_opened_ledger("worker-1", "#359", Some("merge_error:conflict"));
    assert_eq!(
        merge_lifecycle(&ledger, "worker-1"),
        Some(MergeLifecycle::ChecksFailing)
    );
}

#[test]
fn a_cleared_hold_on_an_open_pr_reads_as_waiting_on_the_judge() {
    // `overseer::daemon::merge` clears `merge_hold` on `Outcome::Pending`
    // (the judge queued the request but has not answered yet) — the
    // deterministic gate has nothing left to hold on, so an unset reason
    // on a still-open pull request means the judge is what is pending.
    let ledger = pr_opened_ledger("worker-1", "#359", None);
    assert_eq!(
        merge_lifecycle(&ledger, "worker-1"),
        Some(MergeLifecycle::WaitingJudge)
    );
}

#[test]
fn an_unrecognised_hold_reason_falls_back_to_on_hold() {
    let ledger = pr_opened_ledger("worker-1", "#359", Some("behind_branch_updated"));
    assert_eq!(
        merge_lifecycle(&ledger, "worker-1"),
        Some(MergeLifecycle::OnHold)
    );
}

#[test]
fn a_merged_pull_request_has_no_lifecycle_of_its_own() {
    // Genuinely finished; the plain `Status::Done` glyph already tells the
    // truth, so `merge_lifecycle` stays out of the way.
    let mut ledger = escalated_ledger("worker-1", "#359");
    ledger.entries[0].phase = LedgerPhase::Merged;
    assert_eq!(merge_lifecycle(&ledger, "worker-1"), None);
}

#[test]
fn no_ledger_entry_has_no_lifecycle() {
    let ledger = Ledger::default();
    assert_eq!(merge_lifecycle(&ledger, "worker-1"), None);
}

#[test]
fn merge_hold_detail_is_the_raw_reason_verbatim() {
    let ledger = pr_opened_ledger("worker-1", "#359", Some("checks_not_green"));
    assert_eq!(
        merge_hold_detail(Locale::En, &ledger, "worker-1"),
        Some("checks_not_green".into())
    );
}

#[test]
fn merge_hold_detail_names_the_judge_when_no_reason_is_recorded() {
    let ledger = pr_opened_ledger("worker-1", "#359", None);
    assert_eq!(
        merge_hold_detail(Locale::En, &ledger, "worker-1"),
        Some("waiting on merge judge".into())
    );
}

fn decisions(count: usize) -> Vec<DecisionEntry> {
    (0..count)
        .map(|index| {
            decision(
                DecisionKind::Dispatch,
                &format!("#{index}"),
                "worker spawned",
            )
        })
        .collect()
}

fn rendered(decisions: &[DecisionEntry]) -> Vec<String> {
    let mut lines = Vec::new();
    append_decisions(&mut lines, decisions, Locale::En);
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect()
}

fn entry_rows(lines: &[String]) -> usize {
    lines
        .iter()
        .filter(|line| line.starts_with("  · ") || line.starts_with("  ! "))
        .count()
}

#[test]
fn a_complete_list_carries_no_notice() {
    let lines = rendered(&decisions(DETAIL_LIMIT));
    assert_eq!(entry_rows(&lines), DETAIL_LIMIT);
    assert!(!lines.iter().any(|line| line.contains("older entries")));
}

#[test]
fn an_empty_log_says_none_and_stops() {
    let lines = rendered(&[]);
    assert_eq!(lines, ["recent decisions", "  none"]);
}

#[test]
fn a_truncated_list_points_at_the_log_without_counting() {
    // A snapshot filled to its own limit says nothing about how much
    // history sits behind it, so the notice states only that it exists.
    let lines = rendered(&decisions(super::super::DECISION_SNAPSHOT_LIMIT));
    assert_eq!(entry_rows(&lines), DETAIL_LIMIT);
    assert_eq!(
        lines.last().unwrap(),
        "  older entries stay in the decision log"
    );
    assert!(!lines.iter().any(|line| line.contains("more")));
}

#[test]
fn the_newest_decision_is_listed_first() {
    let lines = rendered(&decisions(DETAIL_LIMIT + 1));
    assert!(lines[1].contains(&format!("#{}", DETAIL_LIMIT)));
}

// Split out once this file crossed the 300-line source limit — see the task
// acceptance criteria. `standoffs` is a self-contained reading of the
// decision log, unrelated to the `blocked_reason` / `merge_lifecycle` /
// `append_decisions` coverage above, so it was the natural seam.
#[path = "decisions_standoffs_tests.rs"]
mod standoffs_tests;
