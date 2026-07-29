use chrono::Utc;

use super::*;

fn decision(kind: DecisionKind, task: &str, reason: &str) -> DecisionEntry {
    let mut entry = DecisionEntry::new(kind, reason);
    entry.task = Some(task.into());
    entry
}

fn escalated_ledger(agent_id: &str, task_id: &str) -> Ledger {
    let mut ledger = Ledger::default();
    ledger.entries.push(crate::overseer::ledger::LedgerEntry {
        task_id: task_id.into(),
        display_id: task_id.into(),
        repo: "nantokaworks/robco".into(),
        agent_id: agent_id.into(),
        branch: format!("task-{task_id}"),
        phase: LedgerPhase::Escalated,
        dispatched_at: Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: None,
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        manual_merge_skip: None,
        merge_judge_fail_safes: 0,
        merge_hold_cap_escalated: false,
        merge_hold_rechecks: 0,
        merge_hold_recheck_reason: None,
        merge_hold_recheck_head: None,
    });
    ledger
}

#[test]
fn a_freshly_escalated_worker_needs_a_decision() {
    let ledger = escalated_ledger("worker-1", "#351");
    let decisions = [decision(DecisionKind::Escalate, "#351", "worker blocked")];
    assert_eq!(
        blocked_reason(&ledger, &decisions, "worker-1"),
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
    assert_eq!(blocked_reason(&ledger, &decisions, "worker-1"), None);
}

#[test]
fn a_skipped_case_is_not_flagged() {
    let ledger = escalated_ledger("worker-1", "#351");
    let decisions = [
        decision(DecisionKind::Escalate, "#351", "worker blocked"),
        decision(DecisionKind::Skip, "#351", "not a real blocker"),
    ];
    assert_eq!(blocked_reason(&ledger, &decisions, "worker-1"), None);
}

#[test]
fn a_non_escalated_entry_is_not_flagged() {
    let mut ledger = escalated_ledger("worker-1", "#351");
    ledger.entries[0].phase = LedgerPhase::Working;
    assert_eq!(blocked_reason(&ledger, &[], "worker-1"), None);
}

#[test]
fn an_unrelated_agent_is_not_flagged() {
    let ledger = escalated_ledger("worker-1", "#351");
    assert_eq!(blocked_reason(&ledger, &[], "worker-2"), None);
}

#[test]
fn a_missing_decision_log_still_flags_an_escalated_entry() {
    // The decision log is a bounded tail (`DECISION_SNAPSHOT_LIMIT`); an
    // old escalation can roll out of it while the ledger entry, which has
    // no such cap, is still `Escalated`. Default to still needing a
    // person rather than silently dropping the marker.
    let ledger = escalated_ledger("worker-1", "#351");
    assert_eq!(
        blocked_reason(&ledger, &[], "worker-1"),
        Some("worker blocked".into())
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
    append_decisions(&mut lines, decisions);
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

#[test]
fn an_external_claim_names_the_holder() {
    let decisions = [decision(
        DecisionKind::Hold,
        "#216",
        "claimed_elsewhere:manual-run",
    )];
    assert_eq!(standoffs(&decisions), ["#216 → manual-run"]);
}

#[test]
fn a_later_dispatch_clears_the_standoff() {
    // The operator's manual run finished and the overseer picked the task
    // up; the frame must stop reporting a stand-off that ended.
    let decisions = [
        decision(DecisionKind::Hold, "#216", "claimed_elsewhere:manual-run"),
        decision(DecisionKind::Dispatch, "#216", "worker spawned"),
    ];
    assert!(standoffs(&decisions).is_empty());
}

#[test]
fn a_repeated_standoff_is_reported_once() {
    let decisions = [
        decision(DecisionKind::Hold, "#216", "claimed_elsewhere:manual-run"),
        decision(DecisionKind::Hold, "#216", "claimed_elsewhere:other-agent"),
    ];
    assert_eq!(standoffs(&decisions), ["#216 → other-agent"]);
}

#[test]
fn unrelated_decisions_are_ignored() {
    let decisions = [
        decision(DecisionKind::Skip, "#216", "daily_limit"),
        DecisionEntry::new(DecisionKind::Hold, "claimed_elsewhere:no-task"),
    ];
    assert!(standoffs(&decisions).is_empty());
}
