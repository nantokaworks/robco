use std::cell::RefCell;

use chrono::{TimeZone, Utc};

use super::*;
use crate::{
    config::Config,
    registry::Registry,
    ui::{
        inbox::{InboxItem, InboxKind},
        test_support,
    },
};

fn item(agent_id: &str, session: Option<&str>, second: u32) -> InboxItem {
    InboxItem {
        kind: InboxKind::Escalation,
        repo: Some("robco".into()),
        agent_id: Some(agent_id.into()),
        target_session: session.map(str::to_owned),
        target_id: format!("{agent_id}-{second}"),
        label: format!("{agent_id} — worker"),
        detail: "worker_blocked".into(),
        at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, second).unwrap(),
        pr_url: None,
        pr_facts: None,
        sentence: None,
    }
}

fn worker_app(items: Vec<InboxItem>) -> App {
    let temp = tempfile::tempdir().unwrap();
    let repo = test_support::repo(
        temp.path().join("robco"),
        vec![test_support::agent("worker", temp.path().join("worker"))],
    );
    let config = Config {
        worktree_root: temp.path().into(),
        ..Config::default()
    };
    let mut app = App::new(
        Registry {
            version: 1,
            repos: vec![repo],
        },
        config,
        temp.path().into(),
    );
    app.overseer_visible = false;
    app.orphans.clear();
    app.overseer_inbox = items;
    app.set_repo_expanded(0, true);
    let visible = app.visible();
    app.selected = visible
        .iter()
        .position(|row| matches!(row, Selection::Agent { repo: 0, agent: 0 }))
        .unwrap_or_else(|| panic!("worker row in {visible:?}"));
    app
}

fn lock_overseer_home() -> std::sync::MutexGuard<'static, ()> {
    static STORE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    std::fs::create_dir_all(crate::overseer::overseer_home().unwrap()).unwrap();
    guard
}

#[test]
fn y_approves_the_newest_actionable_escalation_and_suppresses_it() {
    let _store = lock_overseer_home();
    let mut non_actionable = item("worker", None, 2);
    non_actionable.detail = crate::ui::inbox::LEDGER_PARKED_RESUMABLE_MARKER.into();
    let actionable = item("worker", Some("robco_worker"), 1);
    let mut app = worker_app(vec![non_actionable.clone(), actionable.clone()]);
    let calls = RefCell::new(Vec::new());

    assert!(handle_normal_with(
        &mut app,
        KeyCode::Char('y'),
        |app, index| {
            app.approve_inbox_with(index, |session, keys| {
                calls
                    .borrow_mut()
                    .push(format!("{session}:{}", keys.join(",")));
                Ok(())
            });
        },
    ));

    assert_eq!(calls.borrow().as_slice(), ["robco_worker:y,Enter"]);
    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("approval sent")
    );
    assert!(!app.overseer_inbox.iter().any(|item| item == &actionable));
}

#[test]
fn d_dismisses_the_newest_escalation_for_the_worker() {
    let _store = lock_overseer_home();
    let newest = item("worker", None, 2);
    let older = item("worker", Some("robco_worker"), 1);
    let mut app = worker_app(vec![newest.clone(), older.clone()]);

    assert!(handle_normal_with(
        &mut app,
        KeyCode::Char('d'),
        |_, _| panic!("dismiss must not approve"),
    ));

    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("dismissed [ESC] worker-2")
    );
}

#[test]
fn y_and_d_without_an_escalation_only_show_a_message() {
    for code in [KeyCode::Char('y'), KeyCode::Char('d')] {
        let mut app = worker_app(Vec::new());
        let mut approved = false;

        assert!(handle_normal_with(&mut app, code, |_, _| approved = true,));

        assert!(!approved);
        assert_eq!(
            app.message.as_ref().map(|(message, _)| message.as_str()),
            Some("no escalation on this worker")
        );
        assert!(app.overseer_inbox.is_empty());
    }
}

#[test]
fn enter_on_an_agent_is_left_for_attach_routing() {
    let mut app = worker_app(Vec::new());

    assert!(!handle_normal_with(&mut app, KeyCode::Enter, |_, _| {
        panic!("Enter must not approve")
    }));
    assert_eq!(
        app.selected_item(),
        Some(Selection::Agent { repo: 0, agent: 0 })
    );
}

#[test]
fn escalated_ledger_for_a_gone_worker_has_a_repo_row_and_dismisses() {
    use crate::overseer::{
        dismissals::Dismissals,
        ledger::{self, Ledger, LedgerPhase},
        row_summaries::RowSummaries,
    };

    let _store = lock_overseer_home();
    let temp = tempfile::tempdir().unwrap();
    let gone = test_support::agent("agent-approve-ok", temp.path().join("gone"));
    let mut entry = ledger::new_entry(&gone, "robco", Utc::now());
    entry.phase = LedgerPhase::Escalated;
    let repo = test_support::repo(temp.path().join("robco"), vec![]);
    let registry = Registry {
        version: 1,
        repos: vec![repo],
    };
    let inbox = crate::ui::inbox::aggregate(
        &Ledger {
            entries: vec![entry],
            ..Ledger::default()
        },
        &[],
        &[],
        &Dismissals::default(),
        &registry,
        &RowSummaries::default(),
    );
    let mut app = App::new(registry, Config::default(), temp.path().into());
    app.overseer_visible = false;
    app.overseer_inbox_targets = inbox.targets;
    app.overseer_inbox = inbox.items;
    app.set_repo_expanded(0, true);
    app.selected = app
        .visible()
        .iter()
        .position(|row| matches!(row, Selection::RepoEscalation { repo: 0, .. }))
        .expect("repo escalation row");
    let target = app.overseer_inbox[0].target_id.clone();

    assert!(handle_normal(&mut app, KeyCode::Char('d')));
    assert!(
        Dismissals::load()
            .unwrap()
            .entries
            .iter()
            .any(|row| row.target_id == target)
    );
}

#[test]
fn global_decision_has_an_overseer_alert_and_dismisses() {
    use crate::overseer::{
        dismissals::Dismissals,
        ledger::Ledger,
        logging::{DecisionEntry, DecisionKind},
        row_summaries::RowSummaries,
    };

    let _store = lock_overseer_home();
    let temp = tempfile::tempdir().unwrap();
    let decision = DecisionEntry::new(DecisionKind::Escalate, "global alert");
    let registry = Registry::default();
    let inbox = crate::ui::inbox::aggregate(
        &Ledger::default(),
        &[decision],
        &[],
        &Dismissals::default(),
        &registry,
        &RowSummaries::default(),
    );
    let mut app = App::new(registry, Config::default(), temp.path().into());
    app.overseer_visible = true;
    app.orphans.clear();
    app.overseer_inbox_targets = inbox.targets;
    app.overseer_inbox = inbox.items;
    app.selected = app
        .visible()
        .iter()
        .position(|row| matches!(row, Selection::OverseerAlert(_)))
        .expect("overseer alert row");

    let selection = app.selected_item().unwrap();
    let dismissed = RefCell::new(Vec::new());
    assert!(handle_display_only_with(
        &mut app,
        selection,
        0,
        KeyCode::Char('d'),
        |_, index| dismissed.borrow_mut().push(index),
    ));
    assert_eq!(*dismissed.borrow(), [0]);
}

#[test]
fn stale_repo_escalation_index_writes_nothing() {
    let mut app = worker_app(Vec::new());

    assert!(handle_display_only_with(
        &mut app,
        Selection::RepoEscalation { repo: 0, item: 99 },
        99,
        KeyCode::Char('d'),
        |_, _| panic!("stale row must not write"),
    ));

    assert_eq!(
        app.message.as_ref().unwrap().0,
        "inbox item is no longer listed"
    );
}

#[test]
fn repo_escalation_cursor_survives_inbox_reordering() {
    let mut app = worker_app(vec![item("gone", None, 1)]);
    app.registry.repos[0].agents.clear();
    app.selected = app
        .visible()
        .iter()
        .position(|row| matches!(row, Selection::RepoEscalation { .. }))
        .unwrap();
    let identity = app.item_key(app.selected_item().unwrap());
    app.overseer_inbox.insert(0, item("newer-gone", None, 2));

    app.restore_selection(Some(identity.clone()));

    assert_eq!(
        app.selected_item(),
        Some(Selection::RepoEscalation { repo: 0, item: 1 })
    );
    assert_eq!(app.item_key(app.selected_item().unwrap()), identity);
}
