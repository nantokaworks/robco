use super::*;
use crate::{
    config::{Config, Profile},
    dropr::DroprTaskFetch,
    model::{RepoNode, Selection, Status},
    registry::Registry,
    tmux,
    tmux::TmuxServer,
};

fn repo_node(name: &str) -> RepoNode {
    RepoNode {
        path: std::env::temp_dir().join(name),
        name: name.to_string(),
        remote_url: None,
        pinned: true,
        agents: Vec::new(),
        dropr: None,
        dropr_tasks: DroprTaskFetch::default(),
        main_status: None,
        main_last_capture: None,
        main_last_spinner: None,
        main_last_change_at: None,
        main_shell_working: false,
        main_mcp_active: false,
        main_pane_pid: None,
        main_tracked_command: None,
        main_subagents_active: 0,
        main_behind_origin: None,
        checkout_state: None,
    }
}

fn test_app(config: Config) -> App {
    let temp = tempfile::tempdir().unwrap();
    App::new(Registry::default(), config, temp.path().into())
}

fn select_repo(app: &mut App) {
    let index = app
        .visible()
        .iter()
        .position(|selection| matches!(selection, Selection::Repo(0)))
        .unwrap();
    app.selected = index;
}

fn unique_name(case: &str) -> String {
    format!("robco-test-clear-chat-{case}-{}", std::process::id())
}

#[test]
fn without_a_repo_row_selected_it_only_shows_a_message() {
    let mut app = test_app(Config::default());
    app.registry.repos = vec![repo_node("repo")];
    // No agents to select, and the repo row starts collapsed-into-selection
    // by default at index 0 — force a non-repo selection to prove the guard,
    // the same technique `checkout_main_tests` uses for the `c` key.
    app.selected = usize::MAX;

    app.clear_chat_selected();

    assert!(
        app.message
            .as_ref()
            .is_some_and(|(message, _)| message.contains("select a repo"))
    );
    assert!(matches!(app.mode, Mode::Normal));
}

#[test]
fn refuses_when_no_clear_command_is_configured() {
    let config = Config {
        profiles: vec![Profile {
            name: "claude".into(),
            program: "claude".into(),
            autonomous_args: Vec::new(),
            model: None,
            backend: None,
            clear_command: None,
        }],
        ..Config::default()
    };
    let mut app = test_app(config);
    app.registry.repos = vec![repo_node("repo")];
    select_repo(&mut app);

    app.clear_chat_selected();

    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("no clear command configured for claude")
    );
    assert!(matches!(app.mode, Mode::Normal));
}

#[test]
fn refuses_when_there_is_no_live_session() {
    if !tmux::is_installed() {
        eprintln!("skipping: no tmux binary on this runner (GitHub's macos-latest lacks one)");
        return;
    }
    let config = Config {
        tmux_server: TmuxServer::for_tests(),
        ..Config::default()
    };
    let mut app = test_app(config);
    app.registry.repos = vec![repo_node(&unique_name("no-session"))];
    select_repo(&mut app);

    app.clear_chat_selected();

    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("no live chat session to clear")
    );
    assert!(matches!(app.mode, Mode::Normal));
}

#[test]
fn refuses_when_the_session_is_busy() {
    let config = Config {
        tmux_server: TmuxServer::for_tests(),
        ..Config::default()
    };
    let name = unique_name("busy");
    let mut node = repo_node(&name);
    let session = agent::repo_claude_session_name(&config.tmux_session_prefix, &node);
    if tmux::new_session(
        &config.tmux_server,
        &session,
        &std::env::temp_dir(),
        "sh",
        &[],
    )
    .is_err()
    {
        eprintln!("skipping: no usable tmux in this environment");
        return;
    }
    node.main_status = Some(Status::Running);
    let server = config.tmux_server.clone();
    let mut app = test_app(config);
    app.registry.repos = vec![node];
    select_repo(&mut app);

    app.clear_chat_selected();
    let _ = tmux::kill_session(&server, &session);

    assert!(
        app.message
            .as_ref()
            .is_some_and(|(message, _)| message.contains("busy"))
    );
    assert!(matches!(app.mode, Mode::Normal));
}

#[test]
fn idle_session_opens_confirmation_and_sends_the_clear_command_once_confirmed() {
    let config = Config {
        tmux_server: TmuxServer::for_tests(),
        ..Config::default()
    };
    let name = unique_name("idle");
    let mut node = repo_node(&name);
    let session = agent::repo_claude_session_name(&config.tmux_session_prefix, &node);
    if tmux::new_session(
        &config.tmux_server,
        &session,
        &std::env::temp_dir(),
        "sh",
        &[],
    )
    .is_err()
    {
        eprintln!("skipping: no usable tmux in this environment");
        return;
    }
    node.main_status = Some(Status::Idle);
    let path = node.path.clone();
    let server = config.tmux_server.clone();
    let mut app = test_app(config);
    app.registry.repos = vec![node];
    select_repo(&mut app);

    app.clear_chat_selected();
    assert!(matches!(
        &app.mode,
        Mode::ConfirmClearChat { path: dialog_path } if *dialog_path == path
    ));

    app.clear_chat_confirmed(&path);
    let capture = tmux::capture_text(&server, &session);
    let _ = tmux::kill_session(&server, &session);

    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some(format!("cleared chat session for {name}")).as_deref()
    );
    assert!(
        capture.is_ok_and(|text| text.contains("/clear")),
        "expected /clear to reach the session"
    );
}

#[test]
fn a_missing_tmux_binary_is_recognized_by_kind_not_message_text() {
    let missing = crate::Error::Io(std::io::Error::from(std::io::ErrorKind::NotFound));
    assert!(tmux_binary_missing(&missing));
}

#[test]
fn an_ordinary_io_error_is_not_mistaken_for_a_missing_tmux_binary() {
    let denied = crate::Error::Io(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
    assert!(!tmux_binary_missing(&denied));
}

#[test]
fn confirming_a_repo_that_is_gone_reports_it_instead_of_panicking() {
    let mut app = test_app(Config::default());
    app.registry.repos = vec![repo_node("repo")];

    app.clear_chat_confirmed(std::env::temp_dir().join("nowhere").as_path());

    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("repository changed, not cleared")
    );
}
