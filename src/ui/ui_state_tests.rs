use std::path::Path;

use super::*;
use crate::{
    config::Config,
    model::{OverseerCategory, Selection},
    registry::Registry,
    ui::{
        App,
        actions::discovery::path_key,
        test_support::{registry_under, registry_with_agent},
    },
};

/// An app whose UI state is a real file under `dir`, so a second app built the
/// same way sees exactly what a restart would.
fn app_at(dir: &Path, registry: Registry) -> App {
    // The worktree root has to match where the fixture agents live, or the
    // startup prune drops them as unmanaged before any assertion sees them.
    let config = Config {
        worktree_root: dir.join("worktrees"),
        ..Config::default()
    };
    App::new_with_ui_state(
        registry,
        config,
        dir.to_path_buf(),
        UiStateStore::at(dir.join("ui-state.json")),
    )
}

#[test]
fn a_saved_layout_round_trips_through_the_file() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("ui-state.json");

    let mut store = UiStateStore::at(path.clone());
    store.update(|state| {
        state.collapsed_repos.insert("/repos/alpha".into());
        state.expanded_children.insert("/worktrees/one".into());
        state.other_collapsed = true;
        state.orphans_collapsed = true;
        state
            .expanded_overseer_categories
            .insert(OverseerCategory::Inbox.label().to_string());
        state.project_order = vec!["/repos/beta".into(), "/repos/alpha".into()];
    });
    let written = store.state().clone();

    assert_eq!(UiStateStore::at(path).state(), &written);
}

#[test]
fn a_missing_file_reads_as_defaults() {
    let temp = tempfile::tempdir().unwrap();
    let store = UiStateStore::at(temp.path().join("absent.json"));
    assert_eq!(store.state(), &UiState::default());
}

#[test]
fn a_corrupt_file_reads_as_defaults_and_is_left_in_place() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("ui-state.json");
    fs::write(&path, "{ this is not json").unwrap();

    assert_eq!(UiStateStore::at(path.clone()).state(), &UiState::default());
    // Startup must never be blocked by this file, and reading it must not
    // destroy whatever the operator (or another tool) left there.
    assert_eq!(fs::read_to_string(&path).unwrap(), "{ this is not json");
}

#[test]
fn an_in_memory_store_never_writes_a_file() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = UiStateStore::in_memory(UiState::default());
    store.update(|state| state.other_collapsed = true);

    assert!(store.state().other_collapsed);
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn expand_and_collapse_flags_survive_a_restart() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = app_at(temp.path(), registry_under(temp.path(), &["alpha", "beta"]));
    app.set_repo_expanded(1, false);
    app.set_other_collapsed(true);
    app.set_orphans_collapsed(true);
    app.set_overseer_category_expanded(OverseerCategory::Inbox, true);

    let restarted = app_at(temp.path(), registry_under(temp.path(), &["alpha", "beta"]));
    assert_eq!(restarted.expanded, vec![true, false]);
    assert!(restarted.other_collapsed);
    assert!(restarted.orphans_collapsed);
    assert!(restarted.overseer_category_expanded(OverseerCategory::Inbox));
    assert!(!restarted.overseer_category_expanded(OverseerCategory::Health));
}

#[test]
fn a_collapse_is_restored_onto_the_same_repo_after_the_registry_reorders() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = app_at(temp.path(), registry_under(temp.path(), &["alpha", "zeta"]));
    app.set_repo_expanded(1, false);

    // A rescan that inserts a repo ahead of the collapsed one: a positional
    // restore would land the flag on `gamma` instead of on `zeta`.
    let restarted = app_at(
        temp.path(),
        registry_under(temp.path(), &["alpha", "gamma", "zeta"]),
    );
    assert_eq!(restarted.expanded, vec![true, true, false]);
}

#[test]
fn a_repo_that_left_the_registry_is_pruned_from_the_saved_collapse_set() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = app_at(temp.path(), registry_under(temp.path(), &["alpha", "beta"]));
    app.set_repo_expanded(0, false);
    app.set_repo_expanded(1, false);

    // `beta` is gone by the time the next toggle rewrites the set, so its key
    // must not survive to re-collapse a row that returns under the same path.
    let mut app = app_at(temp.path(), registry_under(temp.path(), &["alpha"]));
    app.set_repo_expanded(0, true);

    let saved = UiStateStore::at(temp.path().join("ui-state.json"));
    assert!(
        saved.state().collapsed_repos.is_empty(),
        "{:?}",
        saved.state().collapsed_repos
    );
}

#[test]
fn an_agent_child_expansion_survives_a_restart() {
    let temp = tempfile::tempdir().unwrap();
    let worktree = temp.path().join("worktrees/one");

    let mut app = app_at(temp.path(), registry_with_agent(temp.path()));
    app.set_agent_children_expanded(0, 0, true);
    assert_eq!(
        UiStateStore::at(temp.path().join("ui-state.json"))
            .state()
            .expanded_children
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![path_key(&worktree)]
    );

    let restarted = app_at(temp.path(), registry_with_agent(temp.path()));
    assert!(restarted.agent_children_expanded(0, 0));
}

#[test]
fn a_restored_collapse_hides_the_repos_agent_rows() {
    let temp = tempfile::tempdir().unwrap();
    let mut app = app_at(temp.path(), registry_with_agent(temp.path()));
    assert!(
        app.visible()
            .iter()
            .any(|row| matches!(row, Selection::Agent { .. }))
    );
    app.set_repo_expanded(0, false);

    // The restored flag has to reach `visible()`, not just the field: that is
    // what the operator sees on the next launch.
    let restarted = app_at(temp.path(), registry_with_agent(temp.path()));
    assert!(
        !restarted
            .visible()
            .iter()
            .any(|row| matches!(row, Selection::Agent { .. }))
    );
}
