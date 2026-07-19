use std::fs;

use super::*;
use crate::{config::Config, registry::Registry};

fn test_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    App::new(Registry::default(), Config::default(), temp.path().into())
}

#[test]
fn overseer_visibility_requires_both_daemon_artifacts() {
    let temp = tempfile::tempdir().unwrap();
    let pidfile = temp.path().join("overseer.pid");
    let ledger = temp.path().join("ledger.json");
    assert!(!overseer_artifacts_exist(&pidfile, &ledger));
    fs::write(&pidfile, "123").unwrap();
    assert!(!overseer_artifacts_exist(&pidfile, &ledger));
    fs::write(&ledger, "{}").unwrap();
    assert!(overseer_artifacts_exist(&pidfile, &ledger));
    fs::remove_file(&pidfile).unwrap();
    assert!(!overseer_artifacts_exist(&pidfile, &ledger));
}

#[test]
fn overseer_categories_follow_root_collapse_state() {
    let mut app = test_app();
    app.set_overseer_visibility(true);
    let expected = OverseerCategory::ALL.map(Selection::OverseerCategory);
    assert!(app.visible().windows(4).any(|rows| rows == expected));

    app.set_overseer_collapsed(true);
    assert_eq!(app.visible(), vec![Selection::Overseer]);
}

#[test]
fn app_overseer_frame_height_tracks_content_within_bounds() {
    let mut app = test_app();
    app.set_overseer_visibility(true);
    app.set_overseer_collapsed(true);
    let collapsed_content = crate::ui::tree::overseer_frame::content_lines(&app);
    assert_eq!(
        app.overseer_frame_height(),
        crate::ui::layout::overseer_frame_height(collapsed_content.lines.len())
    );

    app.set_overseer_collapsed(false);
    for category in OverseerCategory::ALL {
        app.set_overseer_category_expanded(category, true);
    }
    assert!(app.overseer_frame_height() > crate::ui::layout::OVERSEER_FRAME_MIN_HEIGHT);
    assert!(app.overseer_frame_height() <= crate::ui::layout::OVERSEER_FRAME_MAX_HEIGHT);
}

#[test]
fn selection_identity_survives_overseer_row_toggle() {
    let temp = tempfile::tempdir().unwrap();
    let repo_path = temp.path().join("repo");
    let repo = serde_json::from_value(serde_json::json!({
        "path": repo_path,
        "name": "repo",
        "remote_url": null,
        "agents": []
    }))
    .unwrap();
    let registry = Registry {
        version: 1,
        repos: vec![repo],
    };
    let mut app = App::new(registry, Config::default(), temp.path().into());
    app.set_overseer_visibility(false);
    assert!(matches!(app.selected_item(), Some(Selection::Repo(0))));

    app.set_overseer_visibility(true);
    assert_eq!(app.selected, 5);
    assert!(matches!(app.selected_item(), Some(Selection::Repo(0))));
    app.move_selection_up();
    assert_eq!(
        app.selected_item(),
        Some(Selection::OverseerCategory(OverseerCategory::Decisions))
    );
    app.selected = 0;
    assert_eq!(app.selected_item(), Some(Selection::Overseer));

    app.set_overseer_visibility(false);
    assert_eq!(app.selected, 0);
    assert!(matches!(app.selected_item(), Some(Selection::Repo(0))));
}
