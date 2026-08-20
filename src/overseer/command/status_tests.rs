use chrono::Utc;

use super::*;
use crate::ui::inbox::{InboxItem, InboxKind};

fn repo(name: &str) -> crate::model::RepoNode {
    let mut repo = crate::discover::repo_node(format!("/tmp/{name}").into(), false);
    repo.name = name.to_string();
    repo
}

fn registry_with(repos: Vec<crate::model::RepoNode>) -> Registry {
    Registry { version: 1, repos }
}

fn answerable_escalation() -> InboxItem {
    InboxItem {
        kind: InboxKind::Escalation,
        repo: None,
        target_session: Some("session".into()),
        target_id: "agent-1".into(),
        label: "agent-1 — worker title".into(),
        detail: "worker_blocked".into(),
        at: Utc::now(),
        pr_url: None,
        pr_facts: None,
        sentence: None,
    }
}

fn watch_only_escalation() -> InboxItem {
    InboxItem {
        kind: InboxKind::Escalation,
        repo: None,
        target_session: Some("session".into()),
        target_id: "#99".into(),
        label: "#99 — checks_waiting".into(),
        detail: "checks_waiting".into(),
        at: Utc::now(),
        pr_url: None,
        pr_facts: None,
        sentence: None,
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
    let items = vec![answerable_escalation(), watch_only_escalation()];
    let reasons = waiting_reasons(&items);
    assert_eq!(reasons.len(), 1);
    assert!(reasons[0].contains("agent-1"));
}

#[test]
fn stuck_summary_reads_as_none_when_nothing_is_broken() {
    assert_eq!(stuck_summary(&[]), "stuck: none");
}

#[test]
fn stuck_reasons_names_an_offline_daemon() {
    let config = OverseerConfig::default();
    let reasons = stuck_reasons(&config, false, None, None, 0);
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
    assert!(stuck_reasons(&config, true, Some(env!("CARGO_PKG_VERSION")), None, 0).is_empty());
}

#[test]
fn running_line_reports_no_active_workers_plainly() {
    let active = ActiveWorkers::default();
    let registry = registry_with(vec![]);
    let line = running_line(&active, &BTreeMap::new(), &registry);
    assert_eq!(line, "running now: no active workers");
}

#[test]
fn running_line_names_repos() {
    let mut active = ActiveWorkers {
        count: 1,
        ..ActiveWorkers::default()
    };
    active.repos.insert("/tmp/robco".into(), 1);
    let registry = registry_with(vec![repo("robco")]);
    let line = running_line(&active, &BTreeMap::new(), &registry);
    assert_eq!(line, "running now: 1 worker(s) (robco=1)");
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
    let registry = registry_with(vec![repo("robco")]);
    let line = running_line(&active, &primary_holders, &registry);
    assert_eq!(line, "running now: 2 worker(s) (robco=2 (primary #452))");
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
