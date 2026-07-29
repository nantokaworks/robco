use std::fs;

use super::*;
use crate::{agent, config::Config, model::ChildWorktree, registry::Registry};

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
fn overseer_categories_are_always_listed_and_the_header_is_not_a_row() {
    let mut app = test_app();
    app.set_overseer_visibility(true);
    // Ignore any live robco tmux sessions the host discovers as orphans so the
    // tree contents are deterministic across environments.
    app.orphans = Vec::new();
    let expected = OverseerCategory::ALL.map(Selection::OverseerCategory);
    assert_eq!(app.visible(), expected.to_vec());
}

fn inbox_item(target_id: &str) -> crate::ui::inbox::InboxItem {
    crate::ui::inbox::InboxItem {
        kind: crate::ui::inbox::InboxKind::Escalation,
        target_session: Some("robco-agent-1".into()),
        target_id: target_id.into(),
        label: format!("{target_id} — escalated"),
        detail: format!("{target_id} escalated"),
        at: chrono::Utc::now(),
    }
}

#[test]
fn expanding_the_inbox_lists_its_items_as_rows_and_collapsing_takes_them_back() {
    let mut app = test_app();
    app.set_overseer_visibility(true);
    app.orphans = Vec::new();
    app.overseer_inbox = vec![inbox_item("#1"), inbox_item("#2")];

    // Collapsed, the categories are still the only OVERSEER rows: an item the
    // operator cannot see is not one the cursor can land on.
    let categories = OverseerCategory::ALL
        .map(Selection::OverseerCategory)
        .to_vec();
    assert_eq!(app.visible(), categories);

    app.set_overseer_category_expanded(OverseerCategory::Inbox, true);
    let inbox_row = OverseerCategory::Inbox.index();
    assert_eq!(
        app.visible(),
        [
            &categories[..=inbox_row],
            &[Selection::OverseerInbox(0), Selection::OverseerInbox(1)],
            &categories[inbox_row + 1..],
        ]
        .concat()
    );

    // Collapsing takes the item rows away and leaves the cursor on a real row
    // rather than past the end of the list — never `None`, and never a stale
    // `OverseerInbox` reference into a row that no longer exists.
    app.selected = inbox_row + 2;
    app.set_overseer_category_expanded(OverseerCategory::Inbox, false);
    assert_eq!(app.visible(), categories);
    assert!(matches!(
        app.selected_item(),
        Some(Selection::OverseerCategory(_))
    ));
}

#[test]
fn app_overseer_frame_height_tracks_content_within_bounds() {
    let mut app = test_app();
    app.set_overseer_visibility(true);
    let content = crate::ui::tree::overseer_frame::content_lines(&app);
    assert_eq!(
        app.overseer_frame_height(),
        crate::ui::layout::overseer_frame_height(content.lines.len())
    );

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
    // Moving up off the first repo row lands on the last OVERSEER category —
    // never on the header, which is no longer a row.
    app.move_selection_up();
    assert_eq!(
        app.selected_item(),
        Some(Selection::OverseerCategory(OverseerCategory::Discord))
    );
    app.selected = 0;
    assert_eq!(
        app.selected_item(),
        Some(Selection::OverseerCategory(OverseerCategory::Inbox))
    );

    app.set_overseer_visibility(false);
    assert_eq!(app.selected, 0);
    assert!(matches!(app.selected_item(), Some(Selection::Repo(0))));
}

#[test]
fn agent_children_default_collapsed_expand_and_hide_when_merged() {
    let temp = tempfile::tempdir().unwrap();
    let repo_path = temp.path().join("repos/repo");
    let owner_path = temp.path().join("worktrees/nex_task-384");
    let slot_path = temp.path().join("worktrees/nex_task-384_slot_snap");
    fs::create_dir_all(&repo_path).unwrap();
    fs::create_dir_all(&owner_path).unwrap();
    fs::create_dir_all(&slot_path).unwrap();
    let mut repo = serde_json::from_value(serde_json::json!({
        "path": repo_path,
        "name": "repo",
        "remote_url": null,
        "agents": []
    }))
    .unwrap();
    let mut owner = agent::adopt_worktree(
        &repo,
        &Config::default(),
        owner_path,
        Some("nex/task-384".into()),
        None,
        None,
        None,
    );
    owner.children.push(ChildWorktree {
        path: slot_path,
        branch: Some("slot/task-386-snap".into()),
        head: None,
        clean: Some(true),
        ahead_behind: Some((0, 1)),
        tmux_session: None,
        modified_at: None,
    });
    repo.agents.push(owner);
    let registry = Registry {
        version: 1,
        repos: vec![repo],
    };
    let config = Config {
        worktree_root: temp.path().join("worktrees"),
        ..Config::default()
    };
    let mut app = App::new(registry, config, temp.path().join("repos"));
    app.set_overseer_visibility(false);

    assert_eq!(app.visible().len(), 2);
    app.set_agent_children_expanded(0, 0, true);
    assert!(matches!(
        app.visible().last(),
        Some(Selection::ChildWorktree { .. })
    ));

    let other_path = temp.path().join("worktrees/nex_task-300");
    fs::create_dir_all(&other_path).unwrap();
    let other = agent::adopt_worktree(
        &app.registry.repos[0],
        &Config::default(),
        other_path,
        Some("nex/task-300".into()),
        None,
        None,
        None,
    );
    app.registry.repos[0].agents.insert(0, other);
    assert!(app.agent_children_expanded(0, 1));

    app.registry.repos[0].agents[1].children[0].ahead_behind = Some((0, 0));
    assert!(
        app.visible()
            .iter()
            .any(|row| matches!(row, Selection::ChildWorktree { .. }))
    );

    app.registry.repos[0].agents[1].children[0].ahead_behind = Some((1, 0));
    assert!(
        !app.visible()
            .iter()
            .any(|row| matches!(row, Selection::ChildWorktree { .. }))
    );

    let recreated = app.registry.repos[0].agents.remove(1);
    app.restore_selection(None);
    app.registry.repos[0].agents.push(recreated);
    assert!(!app.agent_children_expanded(0, 1));
}
