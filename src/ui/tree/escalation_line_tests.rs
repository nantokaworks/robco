use chrono::{Duration, Utc};

use crate::{
    config::Config,
    overseer::{
        ledger::{LedgerEntry, LedgerPhase},
        logging::{DecisionEntry, DecisionKind},
    },
    registry::Registry,
    ui::{
        App,
        inbox::{InboxItem, InboxKind},
        test_support,
        tree::render_test_support::rendered_rows_at_width,
    },
};

fn app() -> App {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::default();
    let agent = test_support::agent("agt-a", config.worktree_root.join("agt-a"));
    let registry = Registry {
        version: 1,
        repos: vec![test_support::repo(temp.path().join("repo"), vec![agent])],
    };
    let mut app = App::new(registry, config, temp.path().into());
    app.orphans.clear();
    app.overseer_visible = false;
    app
}

fn escalation(detail: &str) -> InboxItem {
    InboxItem {
        kind: InboxKind::Escalation,
        repo: Some("repo".into()),
        agent_id: Some("agt-a".into()),
        target_session: None,
        target_id: "#582".into(),
        label: detail.into(),
        detail: detail.into(),
        at: Utc::now(),
        pr_url: None,
        pr_facts: None,
        sentence: None,
    }
}

fn rows(app: &App) -> Vec<String> {
    rendered_rows_at_width(app, 160)
}

#[test]
fn an_escalated_agent_draws_the_item_under_its_row() {
    let mut app = app();
    app.overseer_inbox = vec![escalation("merge_state:dirty")];

    let rows = rows(&app);
    let agent_at = rows.iter().position(|row| row.contains("agt-a")).unwrap();

    assert!(
        rows[agent_at + 1].contains("[ESC] REVIEW — The branch has conflicts"),
        "{rows:?}"
    );
    assert!(!rows[agent_at + 1].contains(" +"), "{rows:?}");
}

#[test]
fn multiple_escalations_draw_only_the_newest_actionable_item_with_a_count() {
    let mut app = app();
    let now = Utc::now();
    let mut older_actionable = escalation("session_auth_failed: expired");
    older_actionable.at = now;
    let mut newest_actionable = escalation("merge_state:dirty");
    newest_actionable.at = now + Duration::seconds(1);
    let mut newer_watch = escalation("checks_waiting");
    newer_watch.at = now + Duration::seconds(2);
    app.overseer_inbox = vec![newer_watch, newest_actionable, older_actionable];

    let rows = rows(&app);
    let escalation_rows = rows
        .iter()
        .filter(|row| row.contains("[ESC]"))
        .collect::<Vec<_>>();

    assert_eq!(escalation_rows.len(), 1, "{rows:?}");
    assert!(
        escalation_rows[0].contains("REVIEW — The branch has conflic")
            && escalation_rows[0].contains(" +2"),
        "{rows:?}"
    );
    assert!(!escalation_rows[0].contains("sign in"), "{rows:?}");
}

#[test]
fn a_healthy_agent_adds_no_line() {
    let rows = rows(&app());
    assert!(!rows.iter().any(|row| row.contains("[ESC]")), "{rows:?}");
}

#[test]
fn a_terminal_reason_duplicated_by_an_escalation_draws_once() {
    let mut app = app();
    app.overseer_inbox = vec![escalation("worker blocked\nmore detail")];
    app.overseer_snapshot
        .ledger
        .entries
        .push(ledger_entry(LedgerPhase::Escalated));
    let mut decision = DecisionEntry::new(DecisionKind::Escalate, "worker blocked");
    decision.task = Some("#582".into());
    app.overseer_snapshot.decisions.push(decision);

    let rows = rows(&app);

    assert_eq!(
        rows.iter()
            .filter(|row| row.contains("worker blocked"))
            .count(),
        1,
        "{rows:?}"
    );
    assert!(!rows.iter().any(|row| row.contains('⚠')), "{rows:?}");
}

#[test]
fn a_humanized_code_reason_duplicated_by_an_escalation_draws_once() {
    let mut app = app();
    app.overseer_inbox = vec![escalation("merge_state:dirty\nmore detail")];
    app.overseer_snapshot
        .ledger
        .entries
        .push(ledger_entry(LedgerPhase::Escalated));
    let mut decision = DecisionEntry::new(DecisionKind::Escalate, "merge_state:dirty");
    decision.task = Some("#582".into());
    app.overseer_snapshot.decisions.push(decision);

    let rows = rows(&app);

    assert_eq!(
        rows.iter()
            .filter(|row| row.contains("The branch has conflicts"))
            .count(),
        1,
        "{rows:?}"
    );
    assert!(!rows.iter().any(|row| row.contains('⚠')), "{rows:?}");
}

#[test]
fn actionable_escalation_adds_the_blocked_badge() {
    let mut app = app();
    app.overseer_inbox = vec![escalation("merge_state:dirty")];
    assert!(
        rows(&app)
            .iter()
            .any(|row| row.contains("agt-a") && row.contains("blocked"))
    );
}

#[test]
fn watch_only_escalation_does_not_add_the_blocked_badge() {
    let mut app = app();
    app.overseer_inbox = vec![escalation("checks_waiting")];
    assert!(
        rows(&app)
            .iter()
            .any(|row| row.contains("agt-a") && !row.contains("blocked"))
    );
}

fn ledger_entry(phase: LedgerPhase) -> LedgerEntry {
    LedgerEntry {
        task_id: "#582".into(),
        dropr_task_id: None,
        display_id: "#582".into(),
        repo: "repo".into(),
        agent_id: "agt-a".into(),
        branch: "robco/agt-a".into(),
        phase,
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
        branch_update_head: None,
    }
}
