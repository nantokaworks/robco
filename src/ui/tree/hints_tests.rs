use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::ui::{App, Mode};

fn empty_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    App::new(
        crate::registry::Registry::default(),
        crate::config::Config::default(),
        temp.path().into(),
    )
}

fn hints_line(
    message: Option<&str>,
    selection: Option<Selection>,
    dropr_task_focus: Option<DroprTaskFocus>,
    reading_task_body: bool,
) -> ratatui::text::Line<'static> {
    super::hints_line(
        &empty_app(),
        message,
        selection,
        dropr_task_focus,
        reading_task_body,
    )
}

#[test]
fn an_empty_tree_offers_only_the_keys_that_work_without_a_row() {
    let line = hints_line(None, None, None, false).to_string();
    assert_eq!(line, "[a] ADD [?] HELP [q] QUIT");
}

#[test]
fn agent_row_advertises_its_own_actions() {
    let line = hints_line(
        None,
        Some(Selection::Agent { repo: 0, agent: 0 }),
        None,
        false,
    )
    .to_string();
    assert_eq!(
        line,
        "[↵] ATTACH [r] RESTART [m] MERGE [u] UPDATE [p] PR [x] REMOVE [?] HELP [q] QUIT"
    );
}

#[test]
fn agent_row_adds_approve_and_dismiss_only_with_an_escalation() {
    let temp = tempfile::tempdir().unwrap();
    let repo = crate::ui::test_support::repo(
        temp.path().join("robco"),
        vec![crate::ui::test_support::agent(
            "worker",
            temp.path().join("worker"),
        )],
    );
    let config = crate::config::Config {
        worktree_root: temp.path().into(),
        ..crate::config::Config::default()
    };
    let mut app = App::new(
        crate::registry::Registry {
            version: 1,
            repos: vec![repo],
        },
        config,
        temp.path().into(),
    );
    let selection = Some(Selection::Agent { repo: 0, agent: 0 });
    assert!(
        !super::hints_line(&app, None, selection, None, false)
            .to_string()
            .contains("APPROVE")
    );

    app.overseer_inbox.push(crate::ui::inbox::InboxItem {
        kind: crate::ui::inbox::InboxKind::Escalation,
        repo: Some("robco".into()),
        agent_id: Some("worker".into()),
        target_session: None,
        target_id: "worker".into(),
        label: "worker".into(),
        detail: "needs user".into(),
        at: chrono::Utc::now(),
        pr_url: None,
        pr_facts: None,
        sentence: None,
    });
    assert_eq!(
        super::hints_line(&app, None, selection, None, false).to_string(),
        "[↵] ATTACH [y] APPROVE [d] DISMISS [r] RESTART [m] MERGE [u] UPDATE [p] PR [x] REMOVE [?] HELP [q] QUIT"
    );
}

#[test]
fn repo_row_advertises_its_own_actions() {
    let line = hints_line(None, Some(Selection::Repo(0)), None, false).to_string();
    assert_eq!(
        line,
        "[n] NEW [a] ADD [r] RELOAD [g] RENAME [?] HELP [q] QUIT"
    );
}

#[test]
fn overseer_ai_row_advertises_attach_and_instruct() {
    let line = hints_line(None, Some(Selection::OverseerAi), None, false).to_string();
    assert_eq!(line, "[↵] ATTACH [i] INSTRUCT [?] HELP [q] QUIT");
}

#[test]
fn remote_chat_rows_advertise_their_actions() {
    let control = hints_line(None, Some(Selection::RemoteControlAi(0)), None, false).to_string();
    let channel = hints_line(
        None,
        Some(Selection::RemoteDiscordChannel {
            host: 0,
            channel: 0,
        }),
        None,
        false,
    )
    .to_string();
    assert_eq!(control, "[↵] ATTACH [i] INSTRUCT [?] HELP [q] QUIT");
    assert_eq!(channel, "[↵] ATTACH [?] HELP [q] QUIT");
}

#[test]
fn discord_category_advertises_expand() {
    let line = hints_line(
        None,
        Some(Selection::OverseerCategory(OverseerCategory::Discord)),
        None,
        false,
    )
    .to_string();
    assert_eq!(line, "[l] EXPAND [?] HELP [q] QUIT");
}

#[test]
fn child_worktree_advertises_attach_only() {
    let line = hints_line(
        None,
        Some(Selection::ChildWorktree {
            repo: 0,
            agent: 0,
            child: 0,
        }),
        None,
        false,
    )
    .to_string();
    assert_eq!(line, "[↵] ATTACH [?] HELP [q] QUIT");
}

#[test]
fn dropr_task_list_focus_advertises_move_open_start_and_back() {
    let line = hints_line(
        None,
        Some(Selection::Repo(0)),
        Some(DroprTaskFocus { task: 0 }),
        false,
    )
    .to_string();
    assert_eq!(
        line,
        "[j/k] MOVE [↵] OPEN [n] START [o] BROWSER [esc] BACK [?] HELP [q] QUIT"
    );
}

#[test]
fn reading_a_task_body_advertises_scroll_start_browser_and_back_only() {
    let line = hints_line(
        None,
        Some(Selection::Repo(0)),
        Some(DroprTaskFocus { task: 0 }),
        true,
    )
    .to_string();
    assert_eq!(line, "[j/k] SCROLL [s] START [o] BROWSER [esc] BACK");
}

#[test]
fn a_repo_row_without_a_drill_down_keeps_its_own_hints() {
    let line = hints_line(None, Some(Selection::Repo(0)), None, false).to_string();
    assert_eq!(
        line,
        "[n] NEW [a] ADD [r] RELOAD [g] RENAME [?] HELP [q] QUIT"
    );
}

#[test]
fn discord_channel_advertises_retry_and_remove() {
    let line = hints_line(None, Some(Selection::DiscordChannel(0)), None, false).to_string();
    assert_eq!(line, "[↵] ATTACH [r] RETRY [x] REMOVE [?] HELP [q] QUIT");
}

#[test]
fn orphan_advertises_attach_and_remove() {
    let line = hints_line(None, Some(Selection::Orphan(0)), None, false).to_string();
    assert_eq!(line, "[↵] ATTACH [x] REMOVE [?] HELP [q] QUIT");
}

#[test]
fn section_headers_advertise_the_fold_they_accept() {
    let line = hints_line(None, Some(Selection::OtherHeader), None, false).to_string();
    assert_eq!(line, "[l] EXPAND [?] HELP [q] QUIT");
    let line = hints_line(None, Some(Selection::OrphanHeader), None, false).to_string();
    assert_eq!(line, "[l] EXPAND [?] HELP [q] QUIT");
}

/// A repository row with one registered repo and nothing else on screen, so
/// row 0 is that repo whatever the host's daemon and tmux sessions are doing.
fn repo_row_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(
        crate::registry::Registry::default(),
        crate::config::Config::default(),
        temp.path().into(),
    );
    app.overseer_visible = false;
    app.orphans = Vec::new();
    app.registry.repos = vec![crate::ui::test_support::repo(
        temp.path().join("alpha"),
        Vec::new(),
    )];
    app.selected = 0;
    assert_eq!(app.selected_item(), Some(Selection::Repo(0)));
    app
}

fn press(app: &mut App, key: &str) -> bool {
    let code = match key {
        "?" => KeyCode::Char('?'),
        other => KeyCode::Char(other.chars().next().expect("empty hint key")),
    };
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
        .expect("key handling failed")
}

/// Every hint on the repository row names a key the row's own handler
/// accepts. Pinned against `App::handle_key` rather than a second hard-coded
/// list, because the bar and the keymap are exactly what drifted apart:
/// dropr:504 dropped the old `g` entry and dropr:505 bound `g` to rename
/// without putting one back, so the row's rename key went unadvertised for
/// two releases. The catch-all arm makes a new hint fail here until it is
/// pinned to what it promises.
#[test]
fn every_repo_row_hint_names_a_key_the_row_accepts() {
    for (key, label) in REPO_HINTS {
        let mut app = repo_row_app();
        let quit = press(&mut app, key);
        match *key {
            "n" => assert!(
                matches!(app.mode, Mode::PromptAgent { .. }),
                "[{key}] {label} did not open the new-agent prompt"
            ),
            "a" => assert!(
                matches!(app.mode, Mode::PromptRepo { .. }),
                "[{key}] {label} did not open the add-repository prompt"
            ),
            "r" => assert!(
                app.message.is_some(),
                "[{key}] {label} said nothing about the reload"
            ),
            "g" => assert!(
                matches!(app.mode, Mode::PromptRenameRepo { .. }),
                "[{key}] {label} did not open the rename prompt"
            ),
            "?" => assert!(
                matches!(app.mode, Mode::Help { .. }),
                "[{key}] {label} did not open the help screen"
            ),
            "q" => assert!(quit, "[{key}] {label} did not quit"),
            other => panic!("repo row hints [{other}] {label} with nothing pinning what it does"),
        }
    }
}
