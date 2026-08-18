use chrono::TimeZone;

use super::*;
use crate::overseer::ledger::LedgerEntry;

fn at(second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, second).unwrap()
}

/// The common case: no dismissals recorded, so aggregation is unfiltered.
fn items(
    ledger: &Ledger,
    decisions: &[DecisionEntry],
    reports: &[AgentQuestionReport],
) -> Vec<InboxItem> {
    aggregate(
        ledger,
        decisions,
        reports,
        &Dismissals::default(),
        &Registry::default(),
    )
    .items
}

fn report(awaiting_confirmation: bool) -> AgentQuestionReport {
    AgentQuestionReport {
        agent_id: "agent-1".into(),
        title: "worker".into(),
        tmux_session: "robco-agent-1".into(),
        status: Status::Waiting,
        awaiting_confirmation,
        at: at(3),
    }
}

fn report_with_status(status: Status) -> AgentQuestionReport {
    AgentQuestionReport {
        status,
        ..report(false)
    }
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
        }],
        ..Ledger::default()
    }
}

fn escalation(reason: &str, second: u32) -> DecisionEntry {
    let mut decision = DecisionEntry::new(DecisionKind::Escalate, reason);
    decision.at = at(second);
    decision.task = Some("task-1".into());
    decision
}

#[test]
fn question_and_live_escalation_are_answerable() {
    let items = items(
        &escalated_ledger(),
        &[escalation("needs user", 2)],
        &[report(true)],
    );

    assert_eq!(items.len(), 2);
    assert_eq!(items[0].kind, InboxKind::Question);
    assert_eq!(items[0].target_session.as_deref(), Some("robco-agent-1"));
    assert_eq!(items[1].kind, InboxKind::Escalation);
    assert_eq!(items[1].target_session.as_deref(), Some("robco-agent-1"));
    assert!(items[1].label.contains("needs user"));
}

#[test]
fn excludes_waiting_agents_without_confirmation_prompt() {
    assert!(items(&Ledger::default(), &[], &[report(false)]).is_empty());
}

#[test]
fn global_and_stale_escalations_are_display_only() {
    let global = DecisionEntry::new(DecisionKind::Escalate, "global alert");
    let stale = items(&escalated_ledger(), &[], &[]);
    let global = items(&Ledger::default(), &[global], &[]);

    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].target_session, None);
    assert_eq!(global.len(), 1);
    assert_eq!(global[0].target_session, None);
    assert_eq!(global[0].target_id, "overseer");
}

/// dropr:444 — a release-pipeline skip (`DecisionKind::Skip`, not
/// `Escalate`) must still surface as an Inbox row: it never had a worker or
/// a live session, so it takes the same global/display-only shape a
/// task-less escalation does.
#[test]
fn a_release_pipeline_skip_is_a_display_only_escalation() {
    let skip = DecisionEntry::new(
        DecisionKind::Skip,
        "release_pipeline_skipped:checkout_not_on_main:operator-wip",
    );
    let rows = items(&Ledger::default(), &[skip], &[]);

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, InboxKind::Escalation);
    assert_eq!(rows[0].target_session, None);
    assert!(
        rows[0]
            .detail
            .contains("release_pipeline_skipped:checkout_not_on_main:operator-wip")
    );
}

/// A plain `daemon`-sourced skip (any other than the release pipeline's) is
/// still not an Inbox concern — only the release-pipeline prefix qualifies.
#[test]
fn an_unrelated_skip_produces_no_inbox_row() {
    let skip = DecisionEntry::new(DecisionKind::Skip, "candidate_circuit_open");
    assert!(items(&Ledger::default(), &[skip], &[]).is_empty());
}

/// The ledger-parked row (`LEDGER_PARKED_MARKER`) reads the phase straight off
/// the ledger with no decision-log tie-breaker at all — unlike the
/// decision-sourced `Escalate` row above, which `monitor::apply::escalate`
/// pairs with a `LogDecision`. So once `monitor::apply::apply_escalation_resolution`
/// (or an explicit `unblocked` report) moves an entry off `Escalated`, the row
/// disappears on the very next aggregation without any of this module's own
/// logic having to know the entry ever escalated in the first place.
#[test]
fn a_resolved_entry_produces_no_ledger_parked_row() {
    let mut ledger = escalated_ledger();
    ledger.entries[0].phase = LedgerPhase::Working;
    ledger.entries[0].settled_at = None;

    assert!(items(&ledger, &[], &[]).is_empty());
}

#[test]
fn a_ledger_parked_escalation_names_the_repo_never_its_absolute_path() {
    // The Inbox row's `label` reaches an operator directly — the CLI's
    // "waiting on you" line prints it verbatim (dropr:357) — so the raw path
    // the ledger records a worker's repository under must never leak through.
    let mut ledger = escalated_ledger();
    ledger.entries[0].repo = "/Users/operator/repos/robco".into();
    let mut repo = crate::discover::repo_node("/Users/operator/repos/robco".into(), false);
    repo.name = "robco".into();
    let registry = Registry {
        version: 1,
        repos: vec![repo],
    };

    let inbox = aggregate(&ledger, &[], &[], &Dismissals::default(), &registry);

    assert_eq!(inbox.items.len(), 1);
    assert!(inbox.items[0].label.contains("robco"));
    assert!(inbox.items[0].detail.contains("robco"));
    assert!(!inbox.items[0].label.contains("/Users/operator"));
    assert!(!inbox.items[0].detail.contains("/Users/operator"));
}

/// An entry the merge pass will still reconsider on its own — see
/// `LedgerEntry::grant_merge_reconsideration` — routes to a distinct,
/// non-actionable remedy rather than `LEDGER_PARKED`'s "merge it by hand".
#[test]
fn a_ledger_parked_escalation_with_a_pull_request_is_marked_resumable() {
    let mut ledger = escalated_ledger();
    ledger.entries[0].pr_url = Some("https://github.com/nantokaworks/robco/pull/1".into());

    let items = items(&ledger, &[], &[]);

    assert_eq!(items.len(), 1);
    let remedy = items[0].remedy();
    assert_eq!(remedy.step, crate::overseer::remedy::Move::Watch);
    assert!(!remedy.actionable());
}

#[test]
fn escalation_requires_a_live_target_session() {
    for status in [Status::Dead, Status::BranchOnly] {
        let items = items(&escalated_ledger(), &[], &[report_with_status(status)]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].target_session, None);
    }

    let items = items(
        &escalated_ledger(),
        &[],
        &[report_with_status(Status::Running)],
    );
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].target_session.as_deref(), Some("robco-agent-1"));
}

#[test]
fn an_item_carries_its_untruncated_reason_for_the_preview() {
    let reason = "merge blocked: the base branch is not protected, and the diff \
                  touches CI configuration";
    let items = items(&Ledger::default(), &[escalation(reason, 2)], &[]);

    assert_eq!(items[0].detail, reason);
}

#[test]
fn a_dismissed_item_is_filtered_out_but_still_counts_as_a_live_target() {
    let mut dismissals = Dismissals::default();
    dismissals.dismiss("ESC", "#159", at(1));

    let inbox = aggregate(
        &escalated_ledger(),
        &[],
        &[],
        &dismissals,
        &Registry::default(),
    );

    assert!(inbox.items.is_empty());
    // The identity has to survive the filter, otherwise the next dismissal's
    // prune pass would drop the entry that is doing the hiding.
    assert!(
        inbox
            .targets
            .contains(&("ESC".to_string(), "#159".to_string()))
    );
}

#[test]
fn a_newer_escalation_for_a_dismissed_target_comes_back() {
    let mut dismissals = Dismissals::default();
    dismissals.dismiss("ESC", "#159", at(1));

    // The ledger row (at(1)) stays hidden; the decision raised afterwards is a
    // new alert about the same target and must be shown. They share an
    // identity, so the newer one is also what survives dedup.
    let inbox = aggregate(
        &escalated_ledger(),
        &[escalation("escalated again", 9)],
        &[],
        &dismissals,
        &Registry::default(),
    );

    assert_eq!(inbox.items.len(), 1);
    assert_eq!(inbox.items[0].detail, "escalated again");
}
