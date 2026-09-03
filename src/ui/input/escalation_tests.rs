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
