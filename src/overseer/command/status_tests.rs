use chrono::Utc;

use super::*;
use crate::model::ManagementMode;
use crate::overseer::ledger::{LedgerEntry, LedgerPhase};
use crate::ui::inbox::{InboxItem, InboxKind};

fn repo(name: &str, management: ManagementMode) -> crate::model::RepoNode {
    let mut repo = crate::discover::repo_node(format!("/tmp/{name}").into(), false);
    repo.name = name.to_string();
    repo.management = management;
    repo
}

fn registry_with(repos: Vec<crate::model::RepoNode>) -> Registry {
    Registry { version: 1, repos }
}

fn ledger_entry(phase: LedgerPhase) -> LedgerEntry {
    LedgerEntry {
        task_id: "task-1".into(),
        display_id: "#12".into(),
        repo: "/tmp/robco".into(),
        agent_id: "agent-1".into(),
        branch: "branch".into(),
        phase,
        dispatched_at: Utc::now(),
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
    }
}

fn answerable_escalation() -> InboxItem {
    InboxItem {
        kind: InboxKind::Escalation,
        target_session: Some("session".into()),
        target_id: "agent-1".into(),
        label: "agent-1 — worker title".into(),
        detail: "worker_blocked".into(),
        at: Utc::now(),
        pr_url: None,
        pr_facts: None,
    }
}

fn watch_only_escalation() -> InboxItem {
    InboxItem {
        kind: InboxKind::Escalation,
        target_session: Some("session".into()),
        target_id: "#99".into(),
        label: "#99 — checks_waiting".into(),
        detail: "checks_waiting".into(),
        at: Utc::now(),
        pr_url: None,
        pr_facts: None,
    }
}

#[test]
fn summarize_reason_leaves_a_short_single_line_reason_untouched() {
    assert_eq!(summarize_reason("checks_waiting"), "checks_waiting");
}

#[test]
fn summarize_reason_keeps_only_the_first_line_of_a_multi_paragraph_verdict() {
    // An escalation reason can run to several lines; the CLI answer has to
    // stay scannable even when the underlying reason does not.
    let verdict = "line one of the verdict\nline two continues here\nline three";
    let summarized = summarize_reason(verdict);
    assert_eq!(summarized, "line one of the verdict…");
    assert!(!summarized.contains("line two"));
}

#[test]
fn summarize_reason_caps_an_overlong_single_line() {
    let reason = "x".repeat(REASON_LINE_LIMIT * 2);
    let summarized = summarize_reason(&reason);
    assert_eq!(summarized.chars().count(), REASON_LINE_LIMIT + 1);
    assert!(summarized.ends_with('…'));
}

#[test]
fn waiting_summary_reads_as_none_when_nothing_needs_a_decision() {
    assert_eq!(waiting_summary(&[]), "waiting on you: none");
}

#[test]
fn waiting_summary_counts_what_it_lists() {
    let reasons = vec!["[ANSWER] agent-1 — worker title".to_string()];
    assert_eq!(waiting_summary(&reasons), "waiting on you: 1");
}

#[test]
fn waiting_reasons_includes_only_actionable_inbox_items() {
    let ledger = Ledger::default();
    let registry = registry_with(vec![]);
    let items = vec![answerable_escalation(), watch_only_escalation()];
    let reasons = waiting_reasons(&ledger, &items, &registry);
    assert_eq!(reasons.len(), 1);
    assert!(reasons[0].contains("agent-1"));
}

#[test]
fn waiting_reasons_names_pull_requests_the_merge_gate_is_holding_for_a_human() {
    // This is the fix for the bug the task exists to close: a raw ledger
    // phase tally cannot tell a pull request still needing a decision apart
    // from a task an operator already resolved elsewhere, but the manual
    // merge skip marker can.
    let mut entry = ledger_entry(LedgerPhase::PrOpened);
    entry.manual_merge_skip = Some("worker manual".into());
    let ledger = Ledger {
        entries: vec![entry],
        ..Ledger::default()
    };
    let registry = registry_with(vec![repo("robco", ManagementMode::Manual)]);
    let reasons = waiting_reasons(&ledger, &[], &registry);
    assert_eq!(reasons.len(), 1);
    assert!(reasons[0].contains("#12"));
    assert!(reasons[0].contains("robco"));
    assert!(!reasons[0].contains("/tmp/robco"));
}

#[test]
fn waiting_reasons_excludes_a_manual_skip_that_already_settled() {
    let mut entry = ledger_entry(LedgerPhase::Merged);
    entry.manual_merge_skip = Some("worker manual".into());
    let ledger = Ledger {
        entries: vec![entry],
        ..Ledger::default()
    };
    let registry = registry_with(vec![]);
    assert!(waiting_reasons(&ledger, &[], &registry).is_empty());
}

#[test]
fn stuck_summary_reads_as_none_when_nothing_is_broken() {
    assert_eq!(stuck_summary(&[]), "stuck: none");
}

#[test]
fn stuck_reasons_names_an_offline_daemon() {
    let config = OverseerConfig::default();
    let reasons = stuck_reasons(&config, false, false, None, None, 0);
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("offline or its heartbeat has gone stale"))
    );
}

#[test]
fn stuck_reasons_is_empty_for_a_healthy_daemon_with_nothing_open() {
    let config = OverseerConfig {
        auto_merge: false,
        ..OverseerConfig::default()
    };
    assert!(
        stuck_reasons(
            &config,
            true,
            false,
            Some(env!("CARGO_PKG_VERSION")),
            None,
            0
        )
        .is_empty()
    );
}

#[test]
fn running_line_reports_no_active_workers_plainly() {
    let active = ActiveWorkers::default();
    let registry = registry_with(vec![]);
    let line = running_line(&active, &BTreeMap::new(), &registry, 3, "10");
    assert_eq!(
        line,
        "running now: no active workers · dispatched today 3/10"
    );
}

#[test]
fn running_line_names_repos() {
    let mut active = ActiveWorkers {
        count: 1,
        ..ActiveWorkers::default()
    };
    active.repos.insert("/tmp/robco".into(), 1);
    let registry = registry_with(vec![repo("robco", ManagementMode::Auto)]);
    let line = running_line(&active, &BTreeMap::new(), &registry, 5, "∞");
    assert_eq!(
        line,
        "running now: 1 worker(s) (robco=1) · dispatched today 5/∞"
    );
    assert!(!line.contains("/tmp"));
}

#[test]
fn running_line_names_the_primary_holder_per_repository() {
    let mut active = ActiveWorkers {
        count: 2,
        ..ActiveWorkers::default()
    };
    active.repos.insert("/tmp/robco".into(), 2);
    let primary_holders = BTreeMap::from([("/tmp/robco".to_string(), "#452".to_string())]);
    let registry = registry_with(vec![repo("robco", ManagementMode::Auto)]);
    let line = running_line(&active, &primary_holders, &registry, 0, "20");
    assert_eq!(
        line,
        "running now: 2 worker(s) (robco=2 (primary #452)) · dispatched today 0/20"
    );
}

#[test]
fn corrupt_lines_warning_is_silent_when_nothing_is_broken() {
    assert_eq!(corrupt_lines_warning(0), None);
}

#[test]
fn corrupt_lines_warning_names_the_count_when_something_is_broken() {
    let warning = corrupt_lines_warning(2).unwrap();
    assert!(warning.starts_with("2 unparseable decision-log line(s)"));
    assert!(warning.contains("decisions.jsonl"));
}
