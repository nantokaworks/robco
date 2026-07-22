use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::{config::Config, registry::Registry};

fn app_with_precheck(
    result: Option<std::result::Result<(), String>>,
) -> (App, Option<mpsc::Sender<std::result::Result<(), String>>>) {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    let (sender, receiver) = mpsc::channel();
    let sender = match result {
        Some(result) => {
            sender.send(result).unwrap();
            drop(sender);
            None
        }
        None => Some(sender),
    };
    app.mode = Mode::ConfirmPr {
        repo_path: "/repo".into(),
        agent_id: "one".into(),
        branch: "feature/one".into(),
        input: "request PR".into(),
    };
    app.pr_precheck_job = Some(PrPrecheckJob {
        repo_path: "/repo".into(),
        agent_id: "one".into(),
        receiver,
    });
    (app, sender)
}

#[test]
fn precheck_ok_keeps_dialog_open() {
    let (mut app, _) = app_with_precheck(Some(Ok(())));

    app.drain_pr_precheck_events();

    assert!(app.pr_precheck_job.is_none());
    assert!(matches!(app.mode, Mode::ConfirmPr { .. }));
    assert!(app.message.is_none());
}

#[test]
fn precheck_err_closes_dialog_and_shows_message() {
    let message = "PR already open for feature/one";
    let (mut app, _) = app_with_precheck(Some(Err(message.to_string())));

    app.drain_pr_precheck_events();

    assert!(app.pr_precheck_job.is_none());
    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some(message)
    );
}

#[test]
fn precheck_disconnected_reports_worker_terminated() {
    let (mut app, sender) = app_with_precheck(None);
    drop(sender);

    app.drain_pr_precheck_events();

    assert!(app.pr_precheck_job.is_none());
    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("PR pre-check worker terminated unexpectedly")
    );
}

#[test]
fn submit_blocked_while_checking() {
    let (mut app, sender) = app_with_precheck(None);
    let mut sent = false;

    app.handle_key_with_pr_sender(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), |_, _| {
        sent = true;
        Ok(())
    })
    .unwrap();

    assert!(!sent);
    assert!(matches!(app.mode, Mode::ConfirmPr { .. }));
    drop(sender);
}

#[test]
fn late_result_from_canceled_worker_ignored_after_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    let (tx_a, rx_a) = mpsc::channel();
    app.mode = Mode::ConfirmPr {
        repo_path: "/repo".into(),
        agent_id: "a".into(),
        branch: "feature/a".into(),
        input: "request PR".into(),
    };
    app.pr_precheck_job = Some(PrPrecheckJob {
        repo_path: "/repo".into(),
        agent_id: "a".into(),
        receiver: rx_a,
    });

    app.pr_precheck_job = None;
    app.mode = Mode::Normal;

    let (tx_b, rx_b) = mpsc::channel();
    app.mode = Mode::ConfirmPr {
        repo_path: "/repo".into(),
        agent_id: "b".into(),
        branch: "feature/b".into(),
        input: "request PR".into(),
    };
    app.pr_precheck_job = Some(PrPrecheckJob {
        repo_path: "/repo".into(),
        agent_id: "b".into(),
        receiver: rx_b,
    });

    let _ = tx_a.send(Err("agent session is not running".to_string()));

    app.drain_pr_precheck_events();

    assert!(matches!(app.mode, Mode::ConfirmPr { .. }));
    assert_eq!(
        app.pr_precheck_job().map(|job| job.agent_id.as_str()),
        Some("b")
    );
    assert!(app.message.is_none());
    drop(tx_b);
}
