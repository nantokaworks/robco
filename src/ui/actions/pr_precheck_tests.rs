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
    app.mode = Mode::PrPrecheck {
        repo_path: "/repo".into(),
        agent_id: "one".into(),
        branch: "feature/one".into(),
        approval_head: None,
    };
    app.pr_precheck_job = Some(PrPrecheckJob { receiver });
    (app, sender)
}

#[test]
fn precheck_ok_opens_confirm_dialog() {
    let (mut app, _) = app_with_precheck(Some(Ok(())));

    app.drain_pr_precheck_events();

    assert!(app.pr_precheck_job.is_none());
    assert!(matches!(
        app.mode,
        Mode::ConfirmPr { ref branch, .. } if branch == "feature/one"
    ));
    assert!(app.message.is_none());
}

#[test]
fn precheck_err_closes_modal_and_shows_message() {
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
fn keys_other_than_esc_are_ignored_while_checking() {
    let (mut app, sender) = app_with_precheck(None);

    app.handle_key_with_pr_sender(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), |_, _| {
        unreachable!("no PR request can be sent while the precheck modal is open")
    })
    .unwrap();

    assert!(matches!(app.mode, Mode::PrPrecheck { .. }));
    assert!(app.pr_precheck_job.is_some());
    drop(sender);
}

#[test]
fn esc_cancels_the_precheck_and_returns_to_normal() {
    let (mut app, sender) = app_with_precheck(None);

    app.handle_key_with_pr_sender(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), |_, _| {
        unreachable!("cancel must not send a PR request")
    })
    .unwrap();

    assert!(matches!(app.mode, Mode::Normal));
    assert!(app.pr_precheck_job.is_none());
    drop(sender);
}

#[test]
fn late_result_from_canceled_worker_ignored_after_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    let (tx_a, rx_a) = mpsc::channel();
    app.mode = Mode::PrPrecheck {
        repo_path: "/repo".into(),
        agent_id: "a".into(),
        branch: "feature/a".into(),
        approval_head: None,
    };
    app.pr_precheck_job = Some(PrPrecheckJob { receiver: rx_a });

    app.pr_precheck_job = None;
    app.mode = Mode::Normal;

    let (tx_b, rx_b) = mpsc::channel();
    app.mode = Mode::PrPrecheck {
        repo_path: "/repo".into(),
        agent_id: "b".into(),
        branch: "feature/b".into(),
        approval_head: None,
    };
    app.pr_precheck_job = Some(PrPrecheckJob { receiver: rx_b });

    let _ = tx_a.send(Err("agent session is not running".to_string()));

    app.drain_pr_precheck_events();

    assert!(matches!(
        app.mode,
        Mode::PrPrecheck { ref agent_id, .. } if agent_id == "b"
    ));
    assert!(app.pr_precheck_job.is_some());
    assert!(app.message.is_none());
    drop(tx_b);
}
