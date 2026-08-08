use super::*;
use crate::{
    config::Config,
    git::test_repo::TestRepo,
    model::{ManagementMode, RepoNode, Selection},
    registry::Registry,
};

fn repo_node(repo: &TestRepo) -> RepoNode {
    RepoNode {
        path: repo.path().to_path_buf(),
        name: "repo".into(),
        remote_url: None,
        // Pinned so `visible()` lists it even though the temp path is not
        // under `App::new`'s `config.repos_root` — see `list::other_location_repos`.
        pinned: true,
        management: ManagementMode::Auto,
        agents: Vec::new(),
        dropr: None,
        dropr_tasks: crate::dropr::DroprTaskFetch::default(),
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

fn test_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    App::new(Registry::default(), Config::default(), temp.path().into())
}

fn select_repo(app: &mut App) {
    let index = app
        .visible()
        .iter()
        .position(|selection| matches!(selection, Selection::Repo(0)))
        .unwrap();
    app.selected = index;
}

#[test]
fn checks_out_main_from_another_clean_branch() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    let mut app = test_app();
    app.registry.repos = vec![repo_node(&repo)];
    select_repo(&mut app);

    app.checkout_main_selected();

    assert_eq!(
        crate::git::current_branch(repo.path()).unwrap().as_deref(),
        Some("main")
    );
    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("checked out main")
    );
    assert_eq!(app.registry.repos[0].checkout_state, None);
}

#[test]
fn refuses_a_dirty_tree_and_leaves_the_branch_untouched() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    std::fs::write(repo.path().join("wip.txt"), "not yet committed").unwrap();
    let mut app = test_app();
    app.registry.repos = vec![repo_node(&repo)];
    select_repo(&mut app);

    app.checkout_main_selected();

    assert_eq!(
        crate::git::current_branch(repo.path()).unwrap().as_deref(),
        Some("task")
    );
    assert!(
        app.message
            .as_ref()
            .is_some_and(|(message, _)| message.contains("checking out main"))
    );
}

#[test]
fn without_a_repo_row_selected_it_only_shows_a_message() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    let mut app = test_app();
    app.registry.repos = vec![repo_node(&repo)];
    // No agents to select, and the repo row starts collapsed-into-selection
    // by default at index 0 — force a non-repo selection to prove the guard.
    app.selected = usize::MAX;

    app.checkout_main_selected();

    assert_eq!(
        crate::git::current_branch(repo.path()).unwrap().as_deref(),
        Some("task")
    );
}

#[test]
fn already_on_main_is_a_harmless_no_op() {
    let repo = TestRepo::new();
    let mut app = test_app();
    app.registry.repos = vec![repo_node(&repo)];
    select_repo(&mut app);

    app.checkout_main_selected();

    assert_eq!(
        crate::git::current_branch(repo.path()).unwrap().as_deref(),
        Some("main")
    );
    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("checked out main")
    );
}

/// Regression guard for the "never automatically" requirement: the
/// background probe (`status::refresh_checkout_branch`, run every discovery
/// tick) only ever reads `HEAD` — it must report the state, not fix it, so a
/// repo left off `main` stays exactly there until the operator presses `c`.
#[test]
fn the_background_probe_never_moves_head() {
    let repo = TestRepo::new();
    repo.feature_branch("task", "task.txt");
    let mut node = crate::discover::repo_node(repo.path().to_path_buf(), false);

    crate::status::refresh_checkout_branch(&mut node);

    assert_eq!(
        node.checkout_state,
        Some(crate::model::CheckoutState::OtherBranch("task".into()))
    );
    assert_eq!(
        crate::git::current_branch(repo.path()).unwrap().as_deref(),
        Some("task")
    );
}
