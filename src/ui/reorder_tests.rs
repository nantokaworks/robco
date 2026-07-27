use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::{
    config::Config,
    registry::Registry,
    ui::{
        test_support::{agent, registry_under, repo},
        ui_state::UiStateStore,
    },
};

fn app_at(dir: &Path, registry: Registry) -> App {
    let config = Config {
        worktree_root: dir.join("worktrees"),
        ..Config::default()
    };
    let mut app = App::new_with_ui_state(
        registry,
        config,
        dir.to_path_buf(),
        UiStateStore::at(dir.join("ui-state.json")),
    );
    // Neither the OVERSEER frame nor whatever robco sessions the host happens
    // to be running has a say in the PROJECTS order under test.
    app.set_overseer_visibility(false);
    app.orphans = Vec::new();
    app
}

/// The project rows in display order, by repo name.
fn project_names(app: &App) -> Vec<String> {
    app.visible()
        .into_iter()
        .filter_map(|row| match row {
            Selection::Repo(idx) => Some(app.registry.repos[idx].name.clone()),
            _ => None,
        })
        .collect()
}

fn select_repo(app: &mut App, name: &str) {
    app.selected = app
        .visible()
        .iter()
        .position(
            |row| matches!(row, Selection::Repo(idx) if app.registry.repos[*idx].name == name),
        )
        .unwrap_or_else(|| panic!("no {name} row"));
    app.restore_preview();
}

fn press(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::SHIFT))
        .unwrap();
}

#[test]
fn shift_down_moves_the_selected_repo_one_slot_and_keeps_the_cursor_on_it() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = app_at(
        temp.path(),
        registry_under(temp.path(), &["alpha", "beta", "gamma"]),
    );
    assert_eq!(project_names(&app), ["alpha", "beta", "gamma"]);

    select_repo(&mut app, "alpha");
    press(&mut app, KeyCode::Down);

    assert_eq!(project_names(&app), ["beta", "alpha", "gamma"]);
    assert_eq!(
        app.selected_item().map(|row| app.item_key(row)),
        Some(app.item_key(Selection::Repo(0))),
        "the cursor did not follow the moved row"
    );
}

#[test]
fn shift_up_moves_the_selected_repo_the_other_way() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = app_at(
        temp.path(),
        registry_under(temp.path(), &["alpha", "beta", "gamma"]),
    );

    select_repo(&mut app, "gamma");
    press(&mut app, KeyCode::Up);

    assert_eq!(project_names(&app), ["alpha", "gamma", "beta"]);
}

#[test]
fn a_move_off_either_end_is_a_no_op_rather_than_a_wrap() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = app_at(
        temp.path(),
        registry_under(temp.path(), &["alpha", "beta", "gamma"]),
    );

    select_repo(&mut app, "alpha");
    press(&mut app, KeyCode::Up);
    assert_eq!(project_names(&app), ["alpha", "beta", "gamma"]);

    select_repo(&mut app, "gamma");
    press(&mut app, KeyCode::Down);
    assert_eq!(project_names(&app), ["alpha", "beta", "gamma"]);
}

#[test]
fn a_non_repo_row_ignores_the_binding() {
    let temp = tempfile::tempdir().unwrap();
    let mut registry = registry_under(temp.path(), &["alpha", "beta"]);
    registry.repos[0]
        .agents
        .push(agent("one", temp.path().join("worktrees/one")));
    let mut app = app_at(temp.path(), registry);

    // The agent row under `alpha`.
    app.selected = app
        .visible()
        .iter()
        .position(|row| matches!(row, Selection::Agent { .. }))
        .expect("no agent row");
    let before = app.selected;
    press(&mut app, KeyCode::Down);

    assert_eq!(project_names(&app), ["alpha", "beta"]);
    assert_eq!(app.selected, before, "the agent row moved the cursor");
}

#[test]
fn a_move_inside_other_locations_stays_inside_it() {
    let temp = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let mut registry = registry_under(temp.path(), &["local"]);
    // Off-launch-dir repos need an agent to be listed at all.
    for name in ["far", "near"] {
        let mut repo = repo(elsewhere.path().join(name), Vec::new());
        repo.agents
            .push(agent(name, temp.path().join("worktrees").join(name)));
        registry.repos.push(repo);
    }
    let mut app = app_at(temp.path(), registry);
    assert_eq!(project_names(&app), ["local", "far", "near"]);

    select_repo(&mut app, "far");
    press(&mut app, KeyCode::Down);
    assert_eq!(project_names(&app), ["local", "near", "far"]);

    // `far` is now last in its section; another press must not pull it up into
    // the local section above the header.
    press(&mut app, KeyCode::Down);
    assert_eq!(project_names(&app), ["local", "near", "far"]);

    // And the local repo cannot fall into the other-locations section either.
    select_repo(&mut app, "local");
    press(&mut app, KeyCode::Down);
    assert_eq!(project_names(&app), ["local", "near", "far"]);
}

#[test]
fn the_chosen_order_survives_a_restart_and_a_discovery_rebuild() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = app_at(
        temp.path(),
        registry_under(temp.path(), &["alpha", "beta", "gamma"]),
    );
    select_repo(&mut app, "gamma");
    press(&mut app, KeyCode::Up);
    press(&mut app, KeyCode::Up);
    assert_eq!(project_names(&app), ["gamma", "alpha", "beta"]);

    // `merge_discovered` rebuilds `repos` alphabetically on every refresh, so
    // this is exactly what the next scan hands the UI.
    let mut registry = Registry::default();
    registry.merge_discovered(registry_under(temp.path(), &["alpha", "beta", "gamma"]).repos);
    assert_eq!(
        registry
            .repos
            .iter()
            .map(|repo| repo.name.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "beta", "gamma"]
    );

    let restarted = app_at(temp.path(), registry);
    assert_eq!(project_names(&restarted), ["gamma", "alpha", "beta"]);
}

#[test]
fn a_newly_discovered_repo_lands_after_the_positioned_ones_alphabetically() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = app_at(temp.path(), registry_under(temp.path(), &["alpha", "beta"]));
    select_repo(&mut app, "beta");
    press(&mut app, KeyCode::Up);
    assert_eq!(project_names(&app), ["beta", "alpha"]);

    // Two repos the saved order has never seen. They sort after everything it
    // does name, alphabetically among themselves, and disturb nothing.
    let grown = app_at(
        temp.path(),
        registry_under(temp.path(), &["alpha", "beta", "delta", "codex"]),
    );
    assert_eq!(project_names(&grown), ["beta", "alpha", "codex", "delta"]);
}

#[test]
fn a_saved_position_for_a_departed_repo_does_not_resurrect_it() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = app_at(
        temp.path(),
        registry_under(temp.path(), &["alpha", "beta", "gamma"]),
    );
    select_repo(&mut app, "gamma");
    press(&mut app, KeyCode::Up);
    assert_eq!(project_names(&app), ["alpha", "gamma", "beta"]);

    // `gamma` is gone from the registry; its saved slot must not conjure a row
    // and must not reorder what is left.
    let shrunk = app_at(temp.path(), registry_under(temp.path(), &["alpha", "beta"]));
    assert_eq!(project_names(&shrunk), ["alpha", "beta"]);
}

#[test]
fn an_unmodified_arrow_still_moves_the_cursor_rather_than_the_row() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = app_at(temp.path(), registry_under(temp.path(), &["alpha", "beta"]));
    select_repo(&mut app, "alpha");

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .unwrap();

    assert_eq!(project_names(&app), ["alpha", "beta"]);
    assert_eq!(app.selected_item(), Some(Selection::Repo(1)));
}
